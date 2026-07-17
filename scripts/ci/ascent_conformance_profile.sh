#!/usr/bin/env bash
# ASCENT conformance profile runner.
#
# The PR profile is a deliberately small, family-representative selector. The
# nightly profile is the complete fs-ascent package with Cargo fail-fast
# disabled. Both profiles are lockfile-closed and enforce an aggregate wall
# budget that intentionally includes compilation. This is an execution guard,
# not a machine-independent performance claim.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd -P)"
readonly REPO_ROOT
readonly CARGO_BIN="${CARGO_BIN:-cargo}"
readonly PR_BUDGET_SECONDS="${FS_ASCENT_PR_BUDGET_SECONDS:-900}"
readonly NIGHTLY_BUDGET_SECONDS="${FS_ASCENT_NIGHTLY_BUDGET_SECONDS:-7200}"
readonly TERMINATION_GRACE_TICKS="${FS_ASCENT_TERMINATION_GRACE_TICKS:-5}"
readonly KILL_DRAIN_TICKS="${FS_ASCENT_KILL_DRAIN_TICKS:-5}"

readonly -a PR_TARGETS=(
  ascent_battery
  constrained_battery
  pareto_battery
  runner_battery
  budget_trend_manifest
  bbob_budget_ledger
)

usage() {
  cat >&2 <<'EOF'
usage:
  scripts/ci/ascent_conformance_profile.sh pr
  scripts/ci/ascent_conformance_profile.sh nightly
  scripts/ci/ascent_conformance_profile.sh --list pr
  scripts/ci/ascent_conformance_profile.sh --list nightly
  scripts/ci/ascent_conformance_profile.sh --self-test

environment:
  CARGO_BIN                         Cargo executable path (default: cargo)
  FS_ASCENT_PR_BUDGET_SECONDS       Aggregate PR wall budget (default: 900)
  FS_ASCENT_NIGHTLY_BUDGET_SECONDS  Aggregate nightly wall budget (default: 7200)

The aggregate wall budget includes compilation and test execution. Callers may
override it for a declared host/target policy; the emitted receipt retains the
effective value. The self-test uses an internal fake workload and clock; it
never invokes Cargo.
EOF
}

require_positive_integer() {
  local label="$1"
  local value="$2"
  if [[ ! "${value}" =~ ^[1-9][0-9]*$ ]]; then
    printf 'invalid %s: expected a positive integer, got %q\n' "${label}" "${value}" >&2
    exit 2
  fi
}

profile_budget() {
  local profile="$1"
  case "${profile}" in
    pr) printf '%s\n' "${PR_BUDGET_SECONDS}" ;;
    nightly) printf '%s\n' "${NIGHTLY_BUDGET_SECONDS}" ;;
    *) return 2 ;;
  esac
}

list_profile() {
  local profile="$1"
  local budget
  budget="$(profile_budget "${profile}")" || {
    usage
    exit 2
  }
  printf '{"schema":"frankensim-ascent-conformance-profile-v1","profile":"%s","budget_seconds":%s,"build_time_included":true}\n' \
    "${profile}" "${budget}"
  if [[ "${profile}" == "pr" ]]; then
    local target
    for target in "${PR_TARGETS[@]}"; do
      printf '{"profile":"pr","package":"fs-ascent","target":"%s","selector":"cargo test --locked -p fs-ascent --test %s -- --nocapture"}\n' \
        "${target}" "${target}"
    done
  else
    printf '%s\n' '{"profile":"nightly","package":"fs-ascent","target":"all","selector":"cargo test --locked -p fs-ascent --no-fail-fast -- --nocapture"}'
  fi
}

emit_target_result() {
  local profile="$1"
  local target="$2"
  local status="$3"
  local elapsed_seconds="$4"
  local launched="$5"
  local exit_code_json="$6"
  local budget_status="$7"
  local drain_status="$8"
  local drained_pid_count="$9"
  emit_line "$(printf '{"schema":"frankensim-ascent-conformance-target-v1","profile":"%s","package":"fs-ascent","target":"%s","status":"%s","elapsed_seconds":%s,"launched":%s,"exit_code":%s,"budget_status":"%s","drain_status":"%s","drained_pid_count":%s}' \
    "${profile}" "${target}" "${status}" "${elapsed_seconds}" "${launched}" \
    "${exit_code_json}" "${budget_status}" "${drain_status}" "${drained_pid_count}")"
}

# Mutable state below is deliberately process-local. It also gives --self-test
# deterministic injection points without temporary files or Cargo processes.
SELF_TEST_CLOCK=0
SELF_TEST_NOW=0
SELF_TEST_WORKLOAD="cargo"
SELF_TEST_CAPTURE=0
SELF_TEST_RECEIPTS=""
SELF_TEST_DESCENDANT_READY=1
CLOCK_NOW=0
ACTIVE_ROOT_PID=""
ACTIVE_PROCESS_GROUP=""
TRACKED_PIDS=()
TARGET_STATUS=""
TARGET_EXIT_CODE_JSON="null"
TARGET_BUDGET_STATUS="within"
TARGET_DRAIN_STATUS="not_needed"
TARGET_DRAINED_PID_COUNT=0
TARGET_ELAPSED_SECONDS=0

emit_line() {
  local line="$1"
  if (( SELF_TEST_CAPTURE != 0 )); then
    SELF_TEST_RECEIPTS+="${line}"$'\n'
  else
    printf '%s\n' "${line}"
  fi
}

clock_now() {
  local observed
  if (( SELF_TEST_CLOCK != 0 )); then
    CLOCK_NOW="${SELF_TEST_NOW}"
    return 0
  fi
  observed="$(date +%s)"
  if [[ ! "${observed}" =~ ^[0-9]+$ ]]; then
    printf 'wall clock returned a non-integer epoch: %q\n' "${observed}" >&2
    return 2
  fi
  CLOCK_NOW="${observed}"
}

pid_is_tracked() {
  local candidate="$1"
  local tracked
  if (( ${#TRACKED_PIDS[@]} == 0 )); then
    return 1
  fi
  for tracked in "${TRACKED_PIDS[@]}"; do
    if [[ "${tracked}" == "${candidate}" ]]; then
      return 0
    fi
  done
  return 1
}

track_pid() {
  local candidate="$1"
  if [[ "${candidate}" =~ ^[1-9][0-9]*$ ]] && ! pid_is_tracked "${candidate}"; then
    TRACKED_PIDS+=("${candidate}")
  fi
}

collect_process_tree() {
  local root_pid="$1"
  local child_pid parent_pid
  local added=1
  track_pid "${root_pid}"
  while (( added != 0 )); do
    added=0
    while read -r child_pid parent_pid; do
      if [[ ! "${child_pid}" =~ ^[1-9][0-9]*$ || ! "${parent_pid}" =~ ^[0-9]+$ ]]; then
        continue
      fi
      if pid_is_tracked "${parent_pid}" && ! pid_is_tracked "${child_pid}"; then
        TRACKED_PIDS+=("${child_pid}")
        added=1
      fi
    done < <(ps -eo pid=,ppid=)
  done
}

pid_is_running() {
  local pid="$1"
  local state
  state="$(ps -o stat= -p "${pid}" 2>/dev/null)" || return 1
  [[ -n "${state}" && "${state}" != *Z* ]]
}

tracked_process_is_running() {
  local pid
  for pid in "${TRACKED_PIDS[@]}"; do
    if pid_is_running "${pid}"; then
      return 0
    fi
  done
  return 1
}

sleep_tick() {
  local attempt
  if (( SELF_TEST_CLOCK != 0 )); then
    if [[ "${SELF_TEST_WORKLOAD}" == "hang" && "${SELF_TEST_DESCENDANT_READY}" == "0" ]]; then
      for ((attempt = 0; attempt < 100; attempt += 1)); do
        collect_process_tree "${ACTIVE_ROOT_PID}"
        if (( ${#TRACKED_PIDS[@]} >= 2 )); then
          SELF_TEST_DESCENDANT_READY=1
          break
        fi
        command sleep 0.01
      done
    fi
    SELF_TEST_NOW=$((SELF_TEST_NOW + 1))
  else
    command sleep 1
  fi
}

launch_workload() {
  local profile="$1"
  local target="$2"
  if [[ "${SELF_TEST_WORKLOAD}" == "pass" ]]; then
    return 0
  fi
  if [[ "${SELF_TEST_WORKLOAD}" == "hang" ]]; then
    command sleep 300 &
    wait
    return $?
  fi
  if [[ "${profile}" == "pr" ]]; then
    exec "${CARGO_BIN}" test --locked -p fs-ascent --test "${target}" -- --nocapture
  fi
  exec "${CARGO_BIN}" test --locked -p fs-ascent --no-fail-fast -- --nocapture
}

start_workload() {
  local profile="$1"
  local target="$2"
  local monitor_was_enabled=0
  local process_group
  if [[ "$-" == *m* ]]; then
    monitor_was_enabled=1
  else
    set -m
  fi
  launch_workload "${profile}" "${target}" &
  ACTIVE_ROOT_PID=$!
  if (( monitor_was_enabled == 0 )); then
    set +m
  fi
  TRACKED_PIDS=()
  track_pid "${ACTIVE_ROOT_PID}"
  process_group="$(ps -o pgid= -p "${ACTIVE_ROOT_PID}" 2>/dev/null)" || process_group=""
  process_group="${process_group//[[:space:]]/}"
  ACTIVE_PROCESS_GROUP=""
  if [[ "${process_group}" == "${ACTIVE_ROOT_PID}" ]]; then
    ACTIVE_PROCESS_GROUP="${process_group}"
  fi
}

signal_owned_tree() {
  local signal_name="$1"
  local pid
  if pid_is_running "${ACTIVE_ROOT_PID}"; then
    collect_process_tree "${ACTIVE_ROOT_PID}"
  fi
  if [[ -n "${ACTIVE_PROCESS_GROUP}" ]]; then
    kill -"${signal_name}" -- "-${ACTIVE_PROCESS_GROUP}" 2>/dev/null || true
  fi
  for pid in "${TRACKED_PIDS[@]}"; do
    kill -"${signal_name}" "${pid}" 2>/dev/null || true
  done
}

terminate_and_drain_owned_tree() {
  local root_pid="${ACTIVE_ROOT_PID}"
  local tick
  local wait_status
  collect_process_tree "${root_pid}"
  signal_owned_tree TERM
  for ((tick = 0; tick < TERMINATION_GRACE_TICKS; tick += 1)); do
    if ! tracked_process_is_running; then
      break
    fi
    signal_owned_tree TERM
    sleep_tick
  done
  if tracked_process_is_running; then
    signal_owned_tree KILL
  fi
  for ((tick = 0; tick < KILL_DRAIN_TICKS; tick += 1)); do
    if ! tracked_process_is_running; then
      break
    fi
    sleep_tick
  done
  if wait "${root_pid}" 2>/dev/null; then
    wait_status=0
  else
    wait_status=$?
  fi
  TARGET_DRAINED_PID_COUNT="${#TRACKED_PIDS[@]}"
  if tracked_process_is_running; then
    TARGET_DRAIN_STATUS="incomplete"
    return 1
  fi
  TARGET_DRAIN_STATUS="complete"
  return "${wait_status}"
}

emit_target_launch() {
  local profile="$1"
  local target="$2"
  local remaining_seconds="$3"
  emit_line "$(printf '{"schema":"frankensim-ascent-conformance-target-v1","event":"launch","profile":"%s","package":"fs-ascent","target":"%s","remaining_budget_seconds":%s}' \
    "${profile}" "${target}" "${remaining_seconds}")"
}

run_target_until_deadline() {
  local profile="$1"
  local target="$2"
  local deadline="$3"
  local started_at finished_at exit_code
  clock_now
  started_at="${CLOCK_NOW}"
  start_workload "${profile}" "${target}"
  while :; do
    if ! pid_is_running "${ACTIVE_ROOT_PID}"; then
      if wait "${ACTIVE_ROOT_PID}" 2>/dev/null; then
        exit_code=0
      else
        exit_code=$?
      fi
      clock_now
      finished_at="${CLOCK_NOW}"
      TARGET_STATUS="pass"
      if (( exit_code != 0 )); then
        TARGET_STATUS="fail"
      fi
      TARGET_EXIT_CODE_JSON="${exit_code}"
      TARGET_BUDGET_STATUS="within"
      TARGET_DRAIN_STATUS="not_needed"
      TARGET_DRAINED_PID_COUNT=0
      TARGET_ELAPSED_SECONDS="$((finished_at - started_at))"
      if (( finished_at > deadline )); then
        TARGET_STATUS="budget_exceeded"
        TARGET_BUDGET_STATUS="exceeded"
      fi
      return 0
    fi
    clock_now
    if (( CLOCK_NOW >= deadline )); then
      TARGET_STATUS="budget_exceeded"
      TARGET_EXIT_CODE_JSON="124"
      TARGET_BUDGET_STATUS="exceeded"
      TARGET_DRAIN_STATUS="incomplete"
      TARGET_DRAINED_PID_COUNT=0
      terminate_and_drain_owned_tree || true
      clock_now
      finished_at="${CLOCK_NOW}"
      TARGET_ELAPSED_SECONDS="$((finished_at - started_at))"
      return 0
    fi
    sleep_tick
  done
}

run_profile() {
  local profile="$1"
  local budget_seconds="$2"
  local started_at deadline finished_at elapsed_seconds target remaining_seconds
  local overall_status=0
  local budget_status="within"
  local run_status="pass"
  local budget_exceeded_target_json="null"
  local run_drain_status="not_needed"
  clock_now
  started_at="${CLOCK_NOW}"
  deadline="$((started_at + budget_seconds))"
  emit_line "$(printf '{"schema":"frankensim-ascent-conformance-run-v1","event":"start","profile":"%s","budget_seconds":%s,"deadline_epoch_seconds":%s,"build_time_included":true,"deadline_enforced":true}' \
    "${profile}" "${budget_seconds}" "${deadline}")"

  if [[ "${profile}" == "pr" ]]; then
    for target in "${PR_TARGETS[@]}"; do
      clock_now
      if (( CLOCK_NOW >= deadline )); then
        budget_status="exceeded"
        overall_status=1
        run_drain_status="not_applicable"
        budget_exceeded_target_json="\"${target}\""
        emit_target_result "${profile}" "${target}" "budget_exceeded" 0 false null \
          "exceeded" "not_applicable" 0
        break
      fi
      remaining_seconds="$((deadline - CLOCK_NOW))"
      emit_target_launch "${profile}" "${target}" "${remaining_seconds}"
      SELF_TEST_DESCENDANT_READY=0
      run_target_until_deadline "${profile}" "${target}" "${deadline}"
      emit_target_result "${profile}" "${target}" "${TARGET_STATUS}" \
        "${TARGET_ELAPSED_SECONDS}" true "${TARGET_EXIT_CODE_JSON}" \
        "${TARGET_BUDGET_STATUS}" "${TARGET_DRAIN_STATUS}" "${TARGET_DRAINED_PID_COUNT}"
      if [[ "${TARGET_STATUS}" == "budget_exceeded" ]]; then
        budget_status="exceeded"
        overall_status=1
        run_drain_status="${TARGET_DRAIN_STATUS}"
        budget_exceeded_target_json="\"${target}\""
        break
      fi
      if [[ "${TARGET_STATUS}" == "fail" ]]; then
        overall_status=1
      fi
    done
  else
    target="all"
    clock_now
    if (( CLOCK_NOW >= deadline )); then
      budget_status="exceeded"
      overall_status=1
      run_drain_status="not_applicable"
      budget_exceeded_target_json='"all"'
      emit_target_result "${profile}" "${target}" "budget_exceeded" 0 false null \
        "exceeded" "not_applicable" 0
    else
      remaining_seconds="$((deadline - CLOCK_NOW))"
      emit_target_launch "${profile}" "${target}" "${remaining_seconds}"
      SELF_TEST_DESCENDANT_READY=0
      run_target_until_deadline "${profile}" "${target}" "${deadline}"
      emit_target_result "${profile}" "${target}" "${TARGET_STATUS}" \
        "${TARGET_ELAPSED_SECONDS}" true "${TARGET_EXIT_CODE_JSON}" \
        "${TARGET_BUDGET_STATUS}" "${TARGET_DRAIN_STATUS}" "${TARGET_DRAINED_PID_COUNT}"
      if [[ "${TARGET_STATUS}" == "budget_exceeded" ]]; then
        budget_status="exceeded"
        overall_status=1
        run_drain_status="${TARGET_DRAIN_STATUS}"
        budget_exceeded_target_json='"all"'
      elif [[ "${TARGET_STATUS}" == "fail" ]]; then
        overall_status=1
      fi
    fi
  fi

  clock_now
  finished_at="${CLOCK_NOW}"
  elapsed_seconds="$((finished_at - started_at))"
  if [[ "${budget_status}" == "exceeded" ]]; then
    run_status="budget_exceeded"
  elif (( overall_status != 0 )); then
    run_status="fail"
  fi
  emit_line "$(printf '{"schema":"frankensim-ascent-conformance-run-v1","event":"finish","profile":"%s","status":"%s","budget_status":"%s","budget_seconds":%s,"elapsed_seconds":%s,"build_time_included":true,"deadline_enforced":true,"budget_exceeded_target":%s,"drain_status":"%s"}' \
    "${profile}" "${run_status}" "${budget_status}" "${budget_seconds}" \
    "${elapsed_seconds}" "${budget_exceeded_target_json}" "${run_drain_status}")"
  return "${overall_status}"
}

self_test_require_contains() {
  local receipt="$1"
  local expected="$2"
  local label="$3"
  if [[ "${receipt}" != *"${expected}"* ]]; then
    printf 'self-test failed (%s): missing %s\n' "${label}" "${expected}" >&2
    return 1
  fi
}

run_self_tests() {
  local status launch_count line
  SELF_TEST_CAPTURE=1
  SELF_TEST_CLOCK=1

  SELF_TEST_NOW=100
  SELF_TEST_WORKLOAD="hang"
  SELF_TEST_RECEIPTS=""
  SELF_TEST_DESCENDANT_READY=0
  if run_profile pr 1; then
    status=0
  else
    status=$?
  fi
  if (( status == 0 )); then
    printf 'self-test failed (deadline): timed-out profile returned success\n' >&2
    return 1
  fi
  if (( SELF_TEST_DESCENDANT_READY == 0 )); then
    printf 'self-test failed (deadline): descendant workload was not observed\n' >&2
    return 1
  fi
  self_test_require_contains "${SELF_TEST_RECEIPTS}" '"target":"ascent_battery","status":"budget_exceeded"' deadline
  self_test_require_contains "${SELF_TEST_RECEIPTS}" '"drain_status":"complete"' drain
  self_test_require_contains "${SELF_TEST_RECEIPTS}" '"event":"finish","profile":"pr","status":"budget_exceeded"' terminal
  launch_count=0
  while IFS= read -r line; do
    if [[ "${line}" == *'"event":"launch"'* ]]; then
      launch_count=$((launch_count + 1))
    fi
  done <<< "${SELF_TEST_RECEIPTS}"
  if (( launch_count != 1 )); then
    printf 'self-test failed (deadline): expected one launch, observed %s\n' "${launch_count}" >&2
    return 1
  fi

  SELF_TEST_NOW=200
  SELF_TEST_WORKLOAD="pass"
  SELF_TEST_RECEIPTS=""
  if run_profile pr 30; then
    status=0
  else
    status=$?
  fi
  if (( status != 0 )); then
    printf 'self-test failed (pass): fake passing profile returned %s\n' "${status}" >&2
    return 1
  fi
  self_test_require_contains "${SELF_TEST_RECEIPTS}" '"event":"finish","profile":"pr","status":"pass"' pass
  if [[ "${SELF_TEST_RECEIPTS}" == *'"status":"budget_exceeded"'* ]]; then
    printf 'self-test failed (pass): passing profile exceeded its fake budget\n' >&2
    return 1
  fi

  SELF_TEST_CAPTURE=0
  printf '%s\n' '{"schema":"frankensim-ascent-conformance-self-test-v1","status":"pass","cases":2,"cargo_invocations":0}'
}

if (( $# == 2 )) && [[ "$1" == "--list" ]]; then
  require_positive_integer "PR budget" "${PR_BUDGET_SECONDS}"
  require_positive_integer "nightly budget" "${NIGHTLY_BUDGET_SECONDS}"
  list_profile "$2"
  exit 0
fi

if (( $# == 1 )) && [[ "$1" == "--self-test" ]]; then
  require_positive_integer "termination grace ticks" "${TERMINATION_GRACE_TICKS}"
  require_positive_integer "kill drain ticks" "${KILL_DRAIN_TICKS}"
  run_self_tests
  exit $?
fi

if (( $# != 1 )) || [[ "$1" != "pr" && "$1" != "nightly" ]]; then
  usage
  exit 2
fi

readonly PROFILE="$1"
BUDGET_SECONDS="$(profile_budget "${PROFILE}")"
readonly BUDGET_SECONDS
require_positive_integer "${PROFILE} budget" "${BUDGET_SECONDS}"
require_positive_integer "termination grace ticks" "${TERMINATION_GRACE_TICKS}"
require_positive_integer "kill drain ticks" "${KILL_DRAIN_TICKS}"

cd "${REPO_ROOT}"
run_profile "${PROFILE}" "${BUDGET_SECONDS}"
exit $?
