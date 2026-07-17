//! Bounded exact-integer topology algebra for I13.2b.
//!
//! The first tranche is an independently replayable Smith-normal-form
//! *witness verifier*. It accepts a general exact integer matrix only when
//! explicit integer left/right transformations and their explicit inverses
//! prove
//!
//! `U * A * V = D`, `U^-1 * U = U * U^-1 = I`, and
//! `V^-1 * V = V * V^-1 = I`.
//!
//! The diagonal must be canonical: nonnegative invariant factors precede
//! zeros and each nonzero factor divides its successor. All arithmetic is
//! checked `i128`; overflow, allocation pressure, work exhaustion, and
//! cancellation refuse without publishing a partially verified value.
//!
//! The second tranche extracts the exact cellular boundary for one admitted
//! [`crate::terminal_relative::TerminalRelativePair`], phase, component, and
//! degree while retaining both canonical quotient bases. It includes the
//! unaugmented zero maps at the two chain-complex edges.
//!
//! The third tranche verifies adjacent pair-bound boundaries and computes
//! `V^-1 * A_(k+1)` for one exact outgoing Smith witness. It refuses a nonzero
//! nonkernel prefix and retains only the witness-relative kernel-coordinate
//! image. Neither value is a terminal-relative homology receipt or physical
//! R3 winding authority. Constructive normal forms and free/torsion quotient
//! decomposition follow in later I13.2b tranches.

use core::fmt;

use crate::terminal_relative::{
    CellRef, ConductorComponentId, IncidenceSign, MAX_TERMINAL_RELATIVE_CELLS,
    MAX_TERMINAL_RELATIVE_INCIDENCES, PhaseId, TerminalRelativePair, TerminalRelativePairId,
};

/// Default maximum row or column extent admitted by the exact checker.
pub const DEFAULT_MAX_MATRIX_EXTENT: usize = 256;
/// Default maximum entries in any one admitted matrix.
pub const DEFAULT_MAX_MATRIX_ENTRIES: usize = DEFAULT_MAX_MATRIX_EXTENT * DEFAULT_MAX_MATRIX_EXTENT;
/// Default maximum entries retained across the source and five witness
/// matrices.
pub const DEFAULT_MAX_RETAINED_ENTRIES: usize = 6 * DEFAULT_MAX_MATRIX_ENTRIES;
/// Default maximum scratch entries used by exact witness multiplication.
pub const DEFAULT_MAX_WORKSPACE_ENTRIES: usize = DEFAULT_MAX_MATRIX_ENTRIES;
/// Exact dot-product terms (one checked multiply/add pair each) admitted by
/// the default checker.
pub const DEFAULT_MAX_SCALAR_OPERATIONS: u128 = 101_000_000;
/// Default maximum component-cell visits across both canonical basis scans.
pub const DEFAULT_MAX_BOUNDARY_COMPONENT_VISITS: usize = 2 * MAX_TERMINAL_RELATIVE_CELLS;
/// Default maximum admitted incidences visited while binding one pair boundary.
pub const DEFAULT_MAX_BOUNDARY_INCIDENCE_VISITS: usize = MAX_TERMINAL_RELATIVE_INCIDENCES;
/// Default retained entries across a pair boundary matrix and its two bases.
pub const DEFAULT_MAX_BOUNDARY_RETAINED_ENTRIES: usize =
    DEFAULT_MAX_MATRIX_ENTRIES + 2 * DEFAULT_MAX_MATRIX_EXTENT;
/// Default retained integer/cell entries for adjacent-boundary transport.
pub const DEFAULT_MAX_KERNEL_RETAINED_ENTRIES: usize =
    10 * DEFAULT_MAX_MATRIX_ENTRIES + 4 * DEFAULT_MAX_MATRIX_EXTENT;
/// Default exact binding comparisons for adjacent-boundary transport.
pub const DEFAULT_MAX_KERNEL_BINDING_ITEMS: usize =
    DEFAULT_MAX_MATRIX_ENTRIES + DEFAULT_MAX_MATRIX_EXTENT;

/// Explicit resource envelope for exact integer witness admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactAlgebraBudget {
    max_rows: usize,
    max_cols: usize,
    max_matrix_entries: usize,
    max_retained_entries: usize,
    max_workspace_entries: usize,
    max_scalar_operations: u128,
}

impl ExactAlgebraBudget {
    /// Construct an exact algebra envelope. Zero limits are valid and admit
    /// only the corresponding empty structures.
    #[must_use]
    pub const fn new(
        max_rows: usize,
        max_cols: usize,
        max_matrix_entries: usize,
        max_retained_entries: usize,
        max_workspace_entries: usize,
        max_scalar_operations: u128,
    ) -> Self {
        Self {
            max_rows,
            max_cols,
            max_matrix_entries,
            max_retained_entries,
            max_workspace_entries,
            max_scalar_operations,
        }
    }

    /// Maximum admitted row count.
    #[must_use]
    pub const fn max_rows(self) -> usize {
        self.max_rows
    }

    /// Maximum admitted column count.
    #[must_use]
    pub const fn max_cols(self) -> usize {
        self.max_cols
    }

    /// Maximum entries in one matrix.
    #[must_use]
    pub const fn max_matrix_entries(self) -> usize {
        self.max_matrix_entries
    }

    /// Maximum entries retained by the source plus complete witness.
    #[must_use]
    pub const fn max_retained_entries(self) -> usize {
        self.max_retained_entries
    }

    /// Maximum internal scratch entries.
    #[must_use]
    pub const fn max_workspace_entries(self) -> usize {
        self.max_workspace_entries
    }

    /// Maximum exact dot-product terms (one checked multiply/add pair each).
    #[must_use]
    pub const fn max_scalar_operations(self) -> u128 {
        self.max_scalar_operations
    }
}

impl Default for ExactAlgebraBudget {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_MATRIX_EXTENT,
            DEFAULT_MAX_MATRIX_EXTENT,
            DEFAULT_MAX_MATRIX_ENTRIES,
            DEFAULT_MAX_RETAINED_ENTRIES,
            DEFAULT_MAX_WORKSPACE_ENTRIES,
            DEFAULT_MAX_SCALAR_OPERATIONS,
        )
    }
}

/// Explicit resource envelope for one terminal-relative boundary extraction.
///
/// Component and incidence visits are capped independently because a sparse
/// admitted complex can be much larger than the requested phase-local matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalRelativeBoundaryBudget {
    max_rows: usize,
    max_cols: usize,
    max_matrix_entries: usize,
    max_retained_entries: usize,
    max_component_visits: usize,
    max_incidence_visits: usize,
}

impl TerminalRelativeBoundaryBudget {
    /// Construct an exact extraction envelope.
    #[must_use]
    pub const fn new(
        max_rows: usize,
        max_cols: usize,
        max_matrix_entries: usize,
        max_retained_entries: usize,
        max_component_visits: usize,
        max_incidence_visits: usize,
    ) -> Self {
        Self {
            max_rows,
            max_cols,
            max_matrix_entries,
            max_retained_entries,
            max_component_visits,
            max_incidence_visits,
        }
    }

    /// Maximum target-basis rows.
    #[must_use]
    pub const fn max_rows(self) -> usize {
        self.max_rows
    }

    /// Maximum source-basis columns.
    #[must_use]
    pub const fn max_cols(self) -> usize {
        self.max_cols
    }

    /// Maximum dense matrix entries.
    #[must_use]
    pub const fn max_matrix_entries(self) -> usize {
        self.max_matrix_entries
    }

    /// Maximum entries retained across matrix and bases.
    #[must_use]
    pub const fn max_retained_entries(self) -> usize {
        self.max_retained_entries
    }

    /// Maximum component-support visits across both canonical basis scans.
    #[must_use]
    pub const fn max_component_visits(self) -> usize {
        self.max_component_visits
    }

    /// Maximum ambient incidence rows visited.
    #[must_use]
    pub const fn max_incidence_visits(self) -> usize {
        self.max_incidence_visits
    }
}

impl Default for TerminalRelativeBoundaryBudget {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_MATRIX_EXTENT,
            DEFAULT_MAX_MATRIX_EXTENT,
            DEFAULT_MAX_MATRIX_ENTRIES,
            DEFAULT_MAX_BOUNDARY_RETAINED_ENTRIES,
            DEFAULT_MAX_BOUNDARY_COMPONENT_VISITS,
            DEFAULT_MAX_BOUNDARY_INCIDENCE_VISITS,
        )
    }
}

/// Resource envelope for transporting an incoming boundary into one verified
/// Smith kernel coordinate system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelCoordinateBudget {
    max_extent: usize,
    max_output_entries: usize,
    max_retained_entries: usize,
    max_binding_items: usize,
    max_scalar_operations: u128,
}

impl KernelCoordinateBudget {
    /// Construct an adjacent-boundary transport envelope.
    #[must_use]
    pub const fn new(
        max_extent: usize,
        max_output_entries: usize,
        max_retained_entries: usize,
        max_binding_items: usize,
        max_scalar_operations: u128,
    ) -> Self {
        Self {
            max_extent,
            max_output_entries,
            max_retained_entries,
            max_binding_items,
            max_scalar_operations,
        }
    }

    /// Maximum admitted outgoing row, shared-chain, or incoming-column extent.
    #[must_use]
    pub const fn max_extent(self) -> usize {
        self.max_extent
    }

    /// Maximum entries retained in the lower kernel-coordinate image.
    #[must_use]
    pub const fn max_output_entries(self) -> usize {
        self.max_output_entries
    }

    /// Maximum retained integer/cell entries across complete input authority
    /// and the lower image. Stable-identity bytes are bounded by pair admission
    /// but are not counted as entries here.
    #[must_use]
    pub const fn max_retained_entries(self) -> usize {
        self.max_retained_entries
    }

    /// Maximum exact source/basis comparisons before multiplication.
    #[must_use]
    pub const fn max_binding_items(self) -> usize {
        self.max_binding_items
    }

    /// Maximum checked dot-product terms in `V^-1 * B`.
    #[must_use]
    pub const fn max_scalar_operations(self) -> u128 {
        self.max_scalar_operations
    }
}

impl Default for KernelCoordinateBudget {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_MATRIX_EXTENT,
            DEFAULT_MAX_MATRIX_ENTRIES,
            DEFAULT_MAX_KERNEL_RETAINED_ENTRIES,
            DEFAULT_MAX_KERNEL_BINDING_ITEMS,
            DEFAULT_MAX_SCALAR_OPERATIONS,
        )
    }
}

/// Dense row-major exact integer matrix with admitted extents.
///
/// Dense storage is intentional in this first witness checker: every retained
/// entry is hard-capped before any checker allocation, and exact product
/// verification visits a deterministic rectangular domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactIntegerMatrix {
    rows: usize,
    cols: usize,
    entries: Vec<i128>,
}

impl ExactIntegerMatrix {
    /// Admit a row-major matrix without normalizing or narrowing any integer.
    pub fn try_new(
        rows: usize,
        cols: usize,
        entries: Vec<i128>,
        budget: ExactAlgebraBudget,
    ) -> Result<Self, IntegralTopologyError> {
        if rows > budget.max_rows || cols > budget.max_cols {
            return Err(IntegralTopologyError::MatrixExtentExceeded {
                rows,
                cols,
                max_rows: budget.max_rows,
                max_cols: budget.max_cols,
            });
        }
        let expected = rows
            .checked_mul(cols)
            .ok_or(IntegralTopologyError::WorkPlanOverflow {
                phase: "matrix entry count",
            })?;
        if expected > budget.max_matrix_entries {
            return Err(IntegralTopologyError::MatrixEntryBudgetExceeded {
                requested: expected,
                max: budget.max_matrix_entries,
            });
        }
        if entries.len() != expected {
            return Err(IntegralTopologyError::MatrixEntryCount {
                rows,
                cols,
                expected,
                actual: entries.len(),
            });
        }
        Ok(Self {
            rows,
            cols,
            entries,
        })
    }

    /// Row count.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Column count.
    #[must_use]
    pub const fn cols(&self) -> usize {
        self.cols
    }

    /// Canonical row-major entries.
    #[must_use]
    pub fn entries(&self) -> &[i128] {
        &self.entries
    }

    /// Exact entry at `(row, col)`, or `None` outside the admitted rectangle.
    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> Option<i128> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        Some(self.entries[row * self.cols + col])
    }

    fn entry(&self, row: usize, col: usize) -> i128 {
        self.entries[row * self.cols + col]
    }

    fn ensure_within(
        &self,
        role: MatrixRole,
        budget: ExactAlgebraBudget,
    ) -> Result<(), IntegralTopologyError> {
        if self.rows > budget.max_rows
            || self.cols > budget.max_cols
            || self.entries.len() > budget.max_matrix_entries
        {
            return Err(IntegralTopologyError::RetainedMatrixExceedsBudget {
                role,
                rows: self.rows,
                cols: self.cols,
                entries: self.entries.len(),
            });
        }
        Ok(())
    }
}

/// Canonically bound terminal-relative boundary matrix.
///
/// Rows are the phase-local quotient basis in degree `k - 1` and columns are
/// the basis in degree `k`. The unaugmented edge maps are represented too:
/// degree zero has no target basis (`0 x dim(C_0)`), while degree one above
/// the complex has no source basis (`dim(C_top) x 0`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRelativeBoundaryMatrix {
    pair: TerminalRelativePairId,
    phase: PhaseId,
    component: ConductorComponentId,
    source_degree: u8,
    target_degree: Option<u8>,
    source_basis: Vec<CellRef>,
    target_basis: Vec<CellRef>,
    matrix: ExactIntegerMatrix,
    work_items: u128,
}

impl TerminalRelativeBoundaryMatrix {
    /// Strong identity of the complete admitted pair.
    #[must_use]
    pub const fn pair_id(&self) -> TerminalRelativePairId {
        self.pair
    }

    /// Phase whose component-local quotient basis was extracted.
    #[must_use]
    pub const fn phase(&self) -> &PhaseId {
        &self.phase
    }

    /// Component bound to the retained phase by pair admission.
    #[must_use]
    pub const fn component(&self) -> &ConductorComponentId {
        &self.component
    }

    /// Source chain degree `k`.
    #[must_use]
    pub const fn source_degree(&self) -> u8 {
        self.source_degree
    }

    /// Target chain degree, or `None` for the unaugmented degree-zero edge.
    #[must_use]
    pub const fn target_degree(&self) -> Option<u8> {
        self.target_degree
    }

    /// Canonically ordered source cells corresponding to matrix columns.
    #[must_use]
    pub fn source_basis(&self) -> &[CellRef] {
        &self.source_basis
    }

    /// Canonically ordered target cells corresponding to matrix rows.
    #[must_use]
    pub fn target_basis(&self) -> &[CellRef] {
        &self.target_basis
    }

    /// Exact row-major boundary matrix.
    #[must_use]
    pub const fn matrix(&self) -> &ExactIntegerMatrix {
        &self.matrix
    }

    /// Deterministic component-cell and incidence visits completed.
    #[must_use]
    pub const fn work_items(&self) -> u128 {
        self.work_items
    }

    /// This binds exact admitted incidence, not a homology or winding result.
    #[must_use]
    pub const fn applicability(&self) -> TopologyApplicability {
        TopologyApplicability::TerminalRelativeIncidenceOnly
    }
}

/// Extract a canonical terminal-relative boundary without injected cancellation.
pub fn extract_terminal_relative_boundary_matrix(
    pair: &TerminalRelativePair,
    phase: &PhaseId,
    source_degree: u8,
    budget: TerminalRelativeBoundaryBudget,
) -> Result<TerminalRelativeBoundaryMatrix, IntegralTopologyError> {
    extract_terminal_relative_boundary_matrix_with_checkpoint(
        pair,
        phase,
        source_degree,
        budget,
        &mut |_| true,
    )
}

/// Extract a canonical pair/phase/degree boundary under bounded polling.
///
/// This is the intrinsic cellular incidence. Terminal-current orientation,
/// port trivialization, and phase-current relabel signs are deliberately not
/// applied. The callback runs before both deterministic component scans,
/// before every ambient incidence visit, and before final publication.
#[allow(clippy::too_many_lines)]
pub fn extract_terminal_relative_boundary_matrix_with_checkpoint(
    pair: &TerminalRelativePair,
    phase: &PhaseId,
    source_degree: u8,
    budget: TerminalRelativeBoundaryBudget,
    checkpoint: &mut impl FnMut(&'static str) -> bool,
) -> Result<TerminalRelativeBoundaryMatrix, IntegralTopologyError> {
    let Some(component_id) = pair.phase_component(phase) else {
        return Err(IntegralTopologyError::UnknownTerminalRelativePhase {
            phase: phase.as_str().to_owned(),
        });
    };
    let max_source_degree = pair.complex().dimension().checked_add(1).ok_or(
        IntegralTopologyError::WorkPlanOverflow {
            phase: "terminal-relative edge degree",
        },
    )?;
    if source_degree > max_source_degree {
        return Err(IntegralTopologyError::BoundaryDegreeOutOfRange {
            degree: source_degree,
            max: max_source_degree,
        });
    }
    let Some(component) = pair
        .components()
        .iter()
        .find(|component| component.id() == component_id)
    else {
        return Err(IntegralTopologyError::PhaseComponentBindingLost {
            phase: phase.as_str().to_owned(),
            component: component_id.as_str().to_owned(),
        });
    };
    let component_cells = component.support().cells().len();
    let component_visits =
        component_cells
            .checked_mul(2)
            .ok_or(IntegralTopologyError::WorkPlanOverflow {
                phase: "terminal-relative component visits",
            })?;
    if component_visits > budget.max_component_visits {
        return Err(IntegralTopologyError::ComponentVisitBudgetExceeded {
            requested: component_visits,
            max: budget.max_component_visits,
        });
    }
    let incidence_visits = pair.complex().incidences().len();
    if incidence_visits > budget.max_incidence_visits {
        return Err(IntegralTopologyError::IncidenceVisitBudgetExceeded {
            requested: incidence_visits,
            max: budget.max_incidence_visits,
        });
    }
    let planned = u128::try_from(component_visits)
        .ok()
        .and_then(|visits| {
            u128::try_from(incidence_visits)
                .ok()
                .and_then(|incidences| visits.checked_add(incidences))
        })
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "terminal-relative boundary extraction",
        })?;
    let retained_phase = phase.clone();
    let retained_component = component_id.clone();
    poll_pair_boundary(
        checkpoint,
        "terminal-relative boundary preflight",
        0,
        planned,
    )?;

    let target_degree = source_degree.checked_sub(1);
    let mut completed = 0_u128;
    let mut source_count = 0_usize;
    let mut target_count = 0_usize;
    for cell in component.support().cells() {
        poll_pair_boundary(
            checkpoint,
            "terminal-relative basis count",
            completed,
            planned,
        )?;
        if !pair.relative().cells().contains(cell) {
            if cell.degree() == source_degree {
                source_count =
                    source_count
                        .checked_add(1)
                        .ok_or(IntegralTopologyError::WorkPlanOverflow {
                            phase: "terminal-relative source basis count",
                        })?;
            }
            if target_degree.is_some_and(|degree| cell.degree() == degree) {
                target_count =
                    target_count
                        .checked_add(1)
                        .ok_or(IntegralTopologyError::WorkPlanOverflow {
                            phase: "terminal-relative target basis count",
                        })?;
            }
        }
        complete_pair_boundary_item(&mut completed)?;
    }
    preflight_boundary_output(target_count, source_count, budget)?;

    let mut source_basis = allocate_cell_refs(source_count, "terminal-relative source basis")?;
    let mut target_basis = allocate_cell_refs(target_count, "terminal-relative target basis")?;
    for cell in component.support().cells() {
        poll_pair_boundary(
            checkpoint,
            "terminal-relative basis materialization",
            completed,
            planned,
        )?;
        if !pair.relative().cells().contains(cell) {
            if cell.degree() == source_degree {
                source_basis.push(*cell);
            }
            if target_degree.is_some_and(|degree| cell.degree() == degree) {
                target_basis.push(*cell);
            }
        }
        complete_pair_boundary_item(&mut completed)?;
    }

    let matrix_entries =
        target_count
            .checked_mul(source_count)
            .ok_or(IntegralTopologyError::WorkPlanOverflow {
                phase: "terminal-relative boundary entries",
            })?;
    let mut entries = allocate_zeroed(matrix_entries, "terminal-relative boundary matrix")?;
    for incidence in pair.complex().incidences() {
        poll_pair_boundary(
            checkpoint,
            "terminal-relative incidence projection",
            completed,
            planned,
        )?;
        if let (Ok(row), Ok(col)) = (
            target_basis.binary_search(&incidence.lower()),
            source_basis.binary_search(&incidence.upper()),
        ) {
            entries[row * source_count + col] = match incidence.sign() {
                IncidenceSign::Negative => -1,
                IncidenceSign::Positive => 1,
            };
        }
        complete_pair_boundary_item(&mut completed)?;
    }
    poll_pair_boundary(
        checkpoint,
        "terminal-relative boundary finalize",
        completed,
        planned,
    )?;
    debug_assert_eq!(completed, planned);

    Ok(TerminalRelativeBoundaryMatrix {
        pair: pair.identity(),
        phase: retained_phase,
        component: retained_component,
        source_degree,
        target_degree,
        source_basis,
        target_basis,
        matrix: ExactIntegerMatrix {
            rows: target_count,
            cols: source_count,
            entries,
        },
        work_items: completed,
    })
}

fn preflight_boundary_output(
    rows: usize,
    cols: usize,
    budget: TerminalRelativeBoundaryBudget,
) -> Result<(), IntegralTopologyError> {
    if rows > budget.max_rows || cols > budget.max_cols {
        return Err(IntegralTopologyError::MatrixExtentExceeded {
            rows,
            cols,
            max_rows: budget.max_rows,
            max_cols: budget.max_cols,
        });
    }
    let matrix_entries = rows
        .checked_mul(cols)
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "terminal-relative boundary entry count",
        })?;
    if matrix_entries > budget.max_matrix_entries {
        return Err(IntegralTopologyError::MatrixEntryBudgetExceeded {
            requested: matrix_entries,
            max: budget.max_matrix_entries,
        });
    }
    let retained = matrix_entries
        .checked_add(rows)
        .and_then(|entries| entries.checked_add(cols))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "terminal-relative boundary retained entries",
        })?;
    if retained > budget.max_retained_entries {
        return Err(IntegralTopologyError::RetainedEntryBudgetExceeded {
            requested: retained,
            max: budget.max_retained_entries,
        });
    }
    Ok(())
}

fn allocate_cell_refs(
    entries: usize,
    phase: &'static str,
) -> Result<Vec<CellRef>, IntegralTopologyError> {
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(entries)
        .map_err(|_| IntegralTopologyError::AllocationRefused {
            phase,
            requested_entries: entries,
        })?;
    Ok(cells)
}

fn complete_pair_boundary_item(completed: &mut u128) -> Result<(), IntegralTopologyError> {
    *completed = completed
        .checked_add(1)
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "completed terminal-relative boundary work",
        })?;
    Ok(())
}

fn poll_pair_boundary(
    checkpoint: &mut impl FnMut(&'static str) -> bool,
    phase: &'static str,
    completed: u128,
    planned: u128,
) -> Result<(), IntegralTopologyError> {
    if checkpoint(phase) {
        Ok(())
    } else {
        Err(IntegralTopologyError::PairBoundaryCancelled {
            phase,
            completed_work_items: completed,
            planned_work_items: planned,
        })
    }
}

/// Exact incoming-boundary image in one verified outgoing Smith kernel basis.
///
/// The retained rows are coordinates in the basis given by the columns
/// `V[:, rank..]` of the exact Smith witness. They are not `CellRef` rows and
/// are not canonical across different valid Smith witnesses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedTerminalRelativeKernelTransport {
    outgoing: TerminalRelativeBoundaryMatrix,
    incoming: TerminalRelativeBoundaryMatrix,
    outgoing_smith: VerifiedSmithNormalForm,
    kernel_image: ExactIntegerMatrix,
    binding_items: usize,
    scalar_operations: u128,
}

impl VerifiedTerminalRelativeKernelTransport {
    /// Pair identity shared by both adjacent boundaries.
    #[must_use]
    pub const fn pair_id(&self) -> TerminalRelativePairId {
        self.outgoing.pair
    }

    /// Shared phase identity.
    #[must_use]
    pub const fn phase(&self) -> &PhaseId {
        &self.outgoing.phase
    }

    /// Shared phase-owned component.
    #[must_use]
    pub const fn component(&self) -> &ConductorComponentId {
        &self.outgoing.component
    }

    /// Homological chain degree `k` whose outgoing map is `A_k`.
    #[must_use]
    pub const fn degree(&self) -> u8 {
        self.outgoing.source_degree
    }

    /// Complete pair-bound outgoing boundary `A_k`.
    #[must_use]
    pub const fn outgoing_boundary(&self) -> &TerminalRelativeBoundaryMatrix {
        &self.outgoing
    }

    /// Complete pair-bound incoming boundary `A_(k+1)`.
    #[must_use]
    pub const fn incoming_boundary(&self) -> &TerminalRelativeBoundaryMatrix {
        &self.incoming
    }

    /// Exact Smith authority that defines the retained kernel coordinates.
    #[must_use]
    pub const fn outgoing_smith(&self) -> &VerifiedSmithNormalForm {
        &self.outgoing_smith
    }

    /// Shared chain-group extent `dim(C_k)`.
    #[must_use]
    pub const fn chain_extent(&self) -> usize {
        self.outgoing.matrix.cols
    }

    /// Verified rank of `A_k`.
    #[must_use]
    pub const fn outgoing_rank(&self) -> usize {
        self.outgoing_smith.rank
    }

    /// Rank of the verified outgoing kernel basis.
    #[must_use]
    pub const fn kernel_dimension(&self) -> usize {
        self.kernel_image.rows
    }

    /// Incoming image in the retained Smith kernel coordinates.
    #[must_use]
    pub const fn kernel_image(&self) -> &ExactIntegerMatrix {
        &self.kernel_image
    }

    /// Exact source/basis comparisons completed before multiplication.
    #[must_use]
    pub const fn binding_items(&self) -> usize {
        self.binding_items
    }

    /// Checked dot-product terms completed for `V^-1 * A_(k+1)`.
    #[must_use]
    pub const fn scalar_operations(&self) -> u128 {
        self.scalar_operations
    }

    /// These are witness-relative kernel coordinates, not homology authority.
    #[must_use]
    pub const fn applicability(&self) -> TopologyApplicability {
        TopologyApplicability::TerminalRelativeKernelCoordinatesOnly
    }
}

/// Verify adjacent pair boundaries and transport the incoming image into the
/// outgoing Smith kernel coordinates without injected cancellation.
#[allow(clippy::large_types_passed_by_value)]
pub fn verify_terminal_relative_kernel_transport(
    outgoing: TerminalRelativeBoundaryMatrix,
    incoming: TerminalRelativeBoundaryMatrix,
    outgoing_smith: VerifiedSmithNormalForm,
    budget: KernelCoordinateBudget,
) -> Result<VerifiedTerminalRelativeKernelTransport, IntegralTopologyError> {
    verify_terminal_relative_kernel_transport_with_checkpoint(
        outgoing,
        incoming,
        outgoing_smith,
        budget,
        &mut |_| true,
    )
}

/// Verify `im(A_(k+1))` lies in `ker(A_k)` and retain its exact coordinates.
///
/// With `U * A_k * V = D`, old and Smith source coordinates satisfy
/// `x = V * y`, so the incoming boundary must be transformed as
/// `V^-1 * A_(k+1)`. Its first `rank(A_k)` rows must vanish exactly. Only the
/// lower rows are retained; the full transformed matrix is never allocated.
#[allow(clippy::too_many_lines)]
#[allow(clippy::large_types_passed_by_value)]
pub fn verify_terminal_relative_kernel_transport_with_checkpoint(
    outgoing: TerminalRelativeBoundaryMatrix,
    incoming: TerminalRelativeBoundaryMatrix,
    outgoing_smith: VerifiedSmithNormalForm,
    budget: KernelCoordinateBudget,
    checkpoint: &mut impl FnMut(&'static str) -> bool,
) -> Result<VerifiedTerminalRelativeKernelTransport, IntegralTopologyError> {
    preflight_adjacent_boundary_bindings(&outgoing, &incoming)?;
    preflight_boundary_internal_shape(&outgoing, "outgoing boundary")?;
    preflight_boundary_internal_shape(&incoming, "incoming boundary")?;

    let outgoing_matrix = outgoing.matrix();
    let incoming_matrix = incoming.matrix();
    let rows = outgoing_matrix.rows;
    let chain_extent = outgoing_matrix.cols;
    let incoming_cols = incoming_matrix.cols;
    if incoming_matrix.rows != chain_extent {
        return Err(IntegralTopologyError::KernelCoordinateInvariantLost {
            field: "adjacent matrix inner extent",
        });
    }
    if outgoing_smith.source.rows != rows || outgoing_smith.source.cols != chain_extent {
        return Err(IntegralTopologyError::OutgoingSmithSourceShapeMismatch {
            expected_rows: rows,
            expected_cols: chain_extent,
            actual_rows: outgoing_smith.source.rows,
            actual_cols: outgoing_smith.source.cols,
        });
    }
    if outgoing_smith.rank > chain_extent {
        return Err(IntegralTopologyError::KernelCoordinateInvariantLost {
            field: "outgoing Smith rank exceeds chain extent",
        });
    }
    if rows > budget.max_extent
        || chain_extent > budget.max_extent
        || incoming_cols > budget.max_extent
    {
        return Err(IntegralTopologyError::KernelCoordinateExtentExceeded {
            outgoing_rows: rows,
            chain_extent,
            incoming_cols,
            max: budget.max_extent,
        });
    }

    let kernel_rows = chain_extent - outgoing_smith.rank;
    let output_entries =
        kernel_rows
            .checked_mul(incoming_cols)
            .ok_or(IntegralTopologyError::WorkPlanOverflow {
                phase: "kernel-coordinate output entries",
            })?;
    if output_entries > budget.max_output_entries {
        return Err(IntegralTopologyError::MatrixEntryBudgetExceeded {
            requested: output_entries,
            max: budget.max_output_entries,
        });
    }
    let binding_items = outgoing
        .source_basis
        .len()
        .checked_add(outgoing_matrix.entries.len())
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "kernel-coordinate binding items",
        })?;
    if binding_items > budget.max_binding_items {
        return Err(IntegralTopologyError::KernelBindingBudgetExceeded {
            requested: binding_items,
            max: budget.max_binding_items,
        });
    }
    let scalar_operations = planned_kernel_scalar_operations(chain_extent, incoming_cols)?;
    if scalar_operations > budget.max_scalar_operations {
        return Err(IntegralTopologyError::ScalarWorkBudgetExceeded {
            requested: scalar_operations,
            max: budget.max_scalar_operations,
        });
    }
    let retained_entries =
        retained_kernel_transport_entries(&outgoing, &incoming, &outgoing_smith, output_entries)?;
    if retained_entries > budget.max_retained_entries {
        return Err(IntegralTopologyError::RetainedEntryBudgetExceeded {
            requested: retained_entries,
            max: budget.max_retained_entries,
        });
    }

    let mut completed_binding = 0_usize;
    let mut completed_scalar = 0_u128;
    poll_kernel_transport(
        checkpoint,
        "kernel-coordinate preflight",
        completed_binding,
        binding_items,
        completed_scalar,
        scalar_operations,
    )?;
    for (index, (outgoing_cell, incoming_cell)) in outgoing
        .source_basis
        .iter()
        .zip(&incoming.target_basis)
        .enumerate()
    {
        poll_kernel_transport(
            checkpoint,
            "kernel-coordinate shared basis",
            completed_binding,
            binding_items,
            completed_scalar,
            scalar_operations,
        )?;
        if outgoing_cell != incoming_cell {
            return Err(IntegralTopologyError::AdjacentBoundaryBasisMismatch {
                index,
                outgoing: Some(*outgoing_cell),
                incoming: Some(*incoming_cell),
            });
        }
        completed_binding += 1;
    }
    for (index, (expected, actual)) in outgoing_matrix
        .entries
        .iter()
        .zip(&outgoing_smith.source.entries)
        .enumerate()
    {
        poll_kernel_transport(
            checkpoint,
            "kernel-coordinate Smith source binding",
            completed_binding,
            binding_items,
            completed_scalar,
            scalar_operations,
        )?;
        if expected != actual {
            return Err(IntegralTopologyError::OutgoingSmithSourceEntryMismatch {
                row: index / chain_extent.max(1),
                col: index % chain_extent.max(1),
                expected: *expected,
                actual: *actual,
            });
        }
        completed_binding += 1;
    }
    debug_assert_eq!(completed_binding, binding_items);

    poll_kernel_transport(
        checkpoint,
        "kernel-coordinate output allocation",
        completed_binding,
        binding_items,
        completed_scalar,
        scalar_operations,
    )?;
    let mut lower = allocate_zeroed(output_entries, "kernel-coordinate incoming image")?;
    for row in 0..chain_extent {
        for col in 0..incoming_cols {
            poll_kernel_transport(
                checkpoint,
                "kernel-coordinate incoming transform",
                completed_binding,
                binding_items,
                completed_scalar,
                scalar_operations,
            )?;
            let coordinate = checked_dot(
                outgoing_smith.right_inverse(),
                row,
                incoming_matrix,
                col,
                SmithWitnessStage::KernelCoordinateIncoming,
                &mut completed_scalar,
            )?;
            if row < outgoing_smith.rank {
                if coordinate != 0 {
                    return Err(IntegralTopologyError::IncomingImageOutsideKernel {
                        row,
                        col,
                        coordinate,
                        invariant_factor: outgoing_smith.invariant_factors[row],
                    });
                }
            } else {
                lower[(row - outgoing_smith.rank) * incoming_cols + col] = coordinate;
            }
        }
    }
    poll_kernel_transport(
        checkpoint,
        "kernel-coordinate finalize",
        completed_binding,
        binding_items,
        completed_scalar,
        scalar_operations,
    )?;
    debug_assert_eq!(completed_scalar, scalar_operations);

    Ok(VerifiedTerminalRelativeKernelTransport {
        outgoing,
        incoming,
        outgoing_smith,
        kernel_image: ExactIntegerMatrix {
            rows: kernel_rows,
            cols: incoming_cols,
            entries: lower,
        },
        binding_items: completed_binding,
        scalar_operations: completed_scalar,
    })
}

fn preflight_adjacent_boundary_bindings(
    outgoing: &TerminalRelativeBoundaryMatrix,
    incoming: &TerminalRelativeBoundaryMatrix,
) -> Result<(), IntegralTopologyError> {
    for (field, matches) in [
        ("pair identity", outgoing.pair == incoming.pair),
        ("phase identity", outgoing.phase == incoming.phase),
        (
            "component identity",
            outgoing.component == incoming.component,
        ),
    ] {
        if !matches {
            return Err(IntegralTopologyError::AdjacentBoundaryBindingMismatch { field });
        }
    }
    let expected_incoming =
        outgoing
            .source_degree
            .checked_add(1)
            .ok_or(IntegralTopologyError::WorkPlanOverflow {
                phase: "adjacent boundary degree",
            })?;
    if incoming.source_degree != expected_incoming
        || incoming.target_degree != Some(outgoing.source_degree)
    {
        return Err(IntegralTopologyError::AdjacentBoundaryDegreeMismatch {
            outgoing: outgoing.source_degree,
            incoming_source: incoming.source_degree,
            incoming_target: incoming.target_degree,
        });
    }
    if outgoing.target_degree != outgoing.source_degree.checked_sub(1) {
        return Err(IntegralTopologyError::KernelCoordinateInvariantLost {
            field: "outgoing target degree",
        });
    }
    if outgoing.source_basis.len() != incoming.target_basis.len() {
        let index = outgoing.source_basis.len().min(incoming.target_basis.len());
        return Err(IntegralTopologyError::AdjacentBoundaryBasisMismatch {
            index,
            outgoing: outgoing.source_basis.get(index).copied(),
            incoming: incoming.target_basis.get(index).copied(),
        });
    }
    Ok(())
}

fn preflight_boundary_internal_shape(
    boundary: &TerminalRelativeBoundaryMatrix,
    field: &'static str,
) -> Result<(), IntegralTopologyError> {
    if boundary.matrix.rows != boundary.target_basis.len()
        || boundary.matrix.cols != boundary.source_basis.len()
    {
        return Err(IntegralTopologyError::KernelCoordinateInvariantLost { field });
    }
    Ok(())
}

fn planned_kernel_scalar_operations(
    chain_extent: usize,
    incoming_cols: usize,
) -> Result<u128, IntegralTopologyError> {
    u128::try_from(chain_extent)
        .ok()
        .and_then(|extent| extent.checked_mul(extent))
        .and_then(|terms| {
            u128::try_from(incoming_cols)
                .ok()
                .and_then(|cols| terms.checked_mul(cols))
        })
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "kernel-coordinate scalar operations",
        })
}

fn retained_kernel_transport_entries(
    outgoing: &TerminalRelativeBoundaryMatrix,
    incoming: &TerminalRelativeBoundaryMatrix,
    smith: &VerifiedSmithNormalForm,
    output_entries: usize,
) -> Result<usize, IntegralTopologyError> {
    [
        outgoing.matrix.entries.len(),
        outgoing.source_basis.len(),
        outgoing.target_basis.len(),
        incoming.matrix.entries.len(),
        incoming.source_basis.len(),
        incoming.target_basis.len(),
        smith.source.entries.len(),
        smith.witness.diagonal.entries.len(),
        smith.witness.left.entries.len(),
        smith.witness.left_inverse.entries.len(),
        smith.witness.right.entries.len(),
        smith.witness.right_inverse.entries.len(),
        smith.invariant_factors.len(),
        output_entries,
    ]
    .into_iter()
    .try_fold(0_usize, |total, entries| total.checked_add(entries))
    .ok_or(IntegralTopologyError::WorkPlanOverflow {
        phase: "kernel-coordinate retained entries",
    })
}

#[allow(clippy::too_many_arguments)]
fn poll_kernel_transport(
    checkpoint: &mut impl FnMut(&'static str) -> bool,
    phase: &'static str,
    completed_binding_items: usize,
    planned_binding_items: usize,
    completed_scalar_operations: u128,
    planned_scalar_operations: u128,
) -> Result<(), IntegralTopologyError> {
    if checkpoint(phase) {
        Ok(())
    } else {
        Err(IntegralTopologyError::KernelCoordinateCancelled {
            phase,
            completed_binding_items,
            planned_binding_items,
            completed_scalar_operations,
            planned_scalar_operations,
        })
    }
}

/// Role of one retained matrix in a Smith witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixRole {
    /// Original exact matrix.
    Source,
    /// Claimed canonical diagonal matrix.
    Diagonal,
    /// Left transformation `U`.
    LeftTransform,
    /// Explicit inverse `U^-1`.
    LeftInverse,
    /// Right transformation `V`.
    RightTransform,
    /// Explicit inverse `V^-1`.
    RightInverse,
}

/// Exact product or inverse identity being checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmithWitnessStage {
    /// `U * U^-1 = I`.
    LeftTimesInverse,
    /// `U^-1 * U = I`.
    LeftInverseTimesTransform,
    /// `V * V^-1 = I`.
    RightTimesInverse,
    /// `V^-1 * V = I`.
    RightInverseTimesTransform,
    /// Intermediate `U * A`.
    LeftTimesSource,
    /// Final `U * A * V = D`.
    DiagonalTransform,
    /// Incoming boundary transformed into outgoing Smith coordinates,
    /// `V^-1 * B`.
    KernelCoordinateIncoming,
}

/// Untrusted complete Smith-normal-form witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmithNormalFormWitness {
    diagonal: ExactIntegerMatrix,
    left: ExactIntegerMatrix,
    left_inverse: ExactIntegerMatrix,
    right: ExactIntegerMatrix,
    right_inverse: ExactIntegerMatrix,
}

impl SmithNormalFormWitness {
    /// Assemble an untrusted witness. Mathematical admission occurs only in
    /// [`verify_smith_normal_form`].
    #[must_use]
    pub const fn new(
        diagonal: ExactIntegerMatrix,
        left: ExactIntegerMatrix,
        left_inverse: ExactIntegerMatrix,
        right: ExactIntegerMatrix,
        right_inverse: ExactIntegerMatrix,
    ) -> Self {
        Self {
            diagonal,
            left,
            left_inverse,
            right,
            right_inverse,
        }
    }

    /// Claimed canonical diagonal.
    #[must_use]
    pub const fn diagonal(&self) -> &ExactIntegerMatrix {
        &self.diagonal
    }

    /// Claimed left transformation.
    #[must_use]
    pub const fn left(&self) -> &ExactIntegerMatrix {
        &self.left
    }

    /// Claimed left inverse.
    #[must_use]
    pub const fn left_inverse(&self) -> &ExactIntegerMatrix {
        &self.left_inverse
    }

    /// Claimed right transformation.
    #[must_use]
    pub const fn right(&self) -> &ExactIntegerMatrix {
        &self.right
    }

    /// Claimed right inverse.
    #[must_use]
    pub const fn right_inverse(&self) -> &ExactIntegerMatrix {
        &self.right_inverse
    }
}

/// Scope intentionally carried by the first exact-algebra tranche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyApplicability {
    /// Algebraic verification only. No physical R3 embedding or winding
    /// conclusion may consume this value as authority.
    AbstractAlgebraOnly,
    /// Exact incidence bound to an admitted pair, phase, component, degree,
    /// and canonical quotient bases. No homology or physical R3 conclusion.
    TerminalRelativeIncidenceOnly,
    /// Exact incoming image in one retained Smith witness's kernel coordinates.
    /// The coordinates are witness-dependent and are not canonical homology.
    TerminalRelativeKernelCoordinatesOnly,
}

/// Authority classification for an unsuccessful exact verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegralTopologyFailureClass {
    /// Exact supplied structure or witness bytes contradict the claimed Smith
    /// decomposition.
    Refuted,
    /// Resource, cancellation, allocation, or arithmetic limits prevented a
    /// mathematical decision.
    Unknown,
}

/// Opaque successfully verified Smith normal form.
///
/// There is no public constructor. The exact source and every witness matrix
/// remain attached so the authority cannot be replayed against another input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSmithNormalForm {
    source: ExactIntegerMatrix,
    witness: SmithNormalFormWitness,
    invariant_factors: Vec<i128>,
    rank: usize,
    scalar_operations: u128,
}

impl VerifiedSmithNormalForm {
    /// Exact source matrix bound to this verification.
    #[must_use]
    pub const fn source(&self) -> &ExactIntegerMatrix {
        &self.source
    }

    /// Canonical diagonal matrix.
    #[must_use]
    pub const fn diagonal(&self) -> &ExactIntegerMatrix {
        &self.witness.diagonal
    }

    /// Verified left transformation.
    #[must_use]
    pub const fn left_transform(&self) -> &ExactIntegerMatrix {
        &self.witness.left
    }

    /// Verified left inverse.
    #[must_use]
    pub const fn left_inverse(&self) -> &ExactIntegerMatrix {
        &self.witness.left_inverse
    }

    /// Verified right transformation.
    #[must_use]
    pub const fn right_transform(&self) -> &ExactIntegerMatrix {
        &self.witness.right
    }

    /// Verified right inverse.
    #[must_use]
    pub const fn right_inverse(&self) -> &ExactIntegerMatrix {
        &self.witness.right_inverse
    }

    /// Positive canonical invariant factors.
    #[must_use]
    pub fn invariant_factors(&self) -> &[i128] {
        &self.invariant_factors
    }

    /// Rank over the integers/rationals, equal to the number of positive
    /// invariant factors.
    #[must_use]
    pub const fn rank(&self) -> usize {
        self.rank
    }

    /// Exact dot-product terms completed by verification. Each term performs
    /// one checked multiplication followed by one checked addition.
    #[must_use]
    pub const fn scalar_operations(&self) -> u128 {
        self.scalar_operations
    }

    /// This first tranche is deliberately not physical topology authority.
    #[must_use]
    pub const fn applicability(&self) -> TopologyApplicability {
        TopologyApplicability::AbstractAlgebraOnly
    }
}

/// Verify a complete Smith witness without injected cancellation.
pub fn verify_smith_normal_form(
    source: ExactIntegerMatrix,
    witness: SmithNormalFormWitness,
    budget: ExactAlgebraBudget,
) -> Result<VerifiedSmithNormalForm, IntegralTopologyError> {
    verify_smith_normal_form_with_checkpoint(source, witness, budget, &mut |_| true)
}

/// Verify a complete Smith witness with bounded cancellation polling.
///
/// After bounded structural preflight, the callback runs before exact work,
/// before each output scalar, and once more before final publication. A
/// callback returning `false` publishes only [`IntegralTopologyError::Cancelled`].
/// Between polls at most `max(rows, cols)` checked dot-product terms execute.
pub fn verify_smith_normal_form_with_checkpoint(
    source: ExactIntegerMatrix,
    witness: SmithNormalFormWitness,
    budget: ExactAlgebraBudget,
    checkpoint: &mut impl FnMut(&'static str) -> bool,
) -> Result<VerifiedSmithNormalForm, IntegralTopologyError> {
    preflight_shapes(&source, &witness, budget)?;
    let rows = source.rows;
    let cols = source.cols;
    let planned = planned_scalar_operations(rows, cols)?;
    if planned > budget.max_scalar_operations {
        return Err(IntegralTopologyError::ScalarWorkBudgetExceeded {
            requested: planned,
            max: budget.max_scalar_operations,
        });
    }
    poll(checkpoint, "smith witness preflight", 0, planned)?;

    let mut completed = 0_u128;
    verify_identity_product(
        &witness.left,
        &witness.left_inverse,
        SmithWitnessStage::LeftTimesInverse,
        checkpoint,
        &mut completed,
        planned,
    )?;
    verify_identity_product(
        &witness.left_inverse,
        &witness.left,
        SmithWitnessStage::LeftInverseTimesTransform,
        checkpoint,
        &mut completed,
        planned,
    )?;
    verify_identity_product(
        &witness.right,
        &witness.right_inverse,
        SmithWitnessStage::RightTimesInverse,
        checkpoint,
        &mut completed,
        planned,
    )?;
    verify_identity_product(
        &witness.right_inverse,
        &witness.right,
        SmithWitnessStage::RightInverseTimesTransform,
        checkpoint,
        &mut completed,
        planned,
    )?;

    let invariant_factors =
        verify_canonical_diagonal(&witness.diagonal, checkpoint, completed, planned)?;
    verify_transform_product(&source, &witness, checkpoint, &mut completed, planned)?;
    poll(checkpoint, "smith witness finalize", completed, planned)?;
    debug_assert_eq!(completed, planned);

    Ok(VerifiedSmithNormalForm {
        source,
        witness,
        rank: invariant_factors.len(),
        invariant_factors,
        scalar_operations: completed,
    })
}

fn verify_transform_product(
    source: &ExactIntegerMatrix,
    witness: &SmithNormalFormWitness,
    checkpoint: &mut impl FnMut(&'static str) -> bool,
    completed: &mut u128,
    planned: u128,
) -> Result<(), IntegralTopologyError> {
    let rows = source.rows;
    let cols = source.cols;
    let workspace = rows
        .checked_mul(cols)
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "smith workspace entries",
        })?;
    let mut left_times_source = allocate_zeroed(workspace, "left-times-source workspace")?;
    for row in 0..rows {
        for col in 0..cols {
            poll(checkpoint, "smith left-times-source", *completed, planned)?;
            left_times_source[row * cols + col] = checked_dot(
                witness.left(),
                row,
                source,
                col,
                SmithWitnessStage::LeftTimesSource,
                completed,
            )?;
        }
    }
    let left_times_source = ExactIntegerMatrix {
        rows,
        cols,
        entries: left_times_source,
    };
    for row in 0..rows {
        for col in 0..cols {
            poll(checkpoint, "smith diagonal transform", *completed, planned)?;
            let actual = checked_dot(
                &left_times_source,
                row,
                witness.right(),
                col,
                SmithWitnessStage::DiagonalTransform,
                completed,
            )?;
            let expected = witness.diagonal().entry(row, col);
            if actual != expected {
                return Err(IntegralTopologyError::WitnessProductMismatch {
                    stage: SmithWitnessStage::DiagonalTransform,
                    row,
                    col,
                    expected,
                    actual,
                });
            }
        }
    }
    Ok(())
}

fn preflight_shapes(
    source: &ExactIntegerMatrix,
    witness: &SmithNormalFormWitness,
    budget: ExactAlgebraBudget,
) -> Result<(), IntegralTopologyError> {
    for (role, matrix) in [
        (MatrixRole::Source, source),
        (MatrixRole::Diagonal, &witness.diagonal),
        (MatrixRole::LeftTransform, &witness.left),
        (MatrixRole::LeftInverse, &witness.left_inverse),
        (MatrixRole::RightTransform, &witness.right),
        (MatrixRole::RightInverse, &witness.right_inverse),
    ] {
        matrix.ensure_within(role, budget)?;
    }

    let rows = source.rows;
    let cols = source.cols;
    require_shape(&witness.diagonal, MatrixRole::Diagonal, rows, cols)?;
    require_shape(&witness.left, MatrixRole::LeftTransform, rows, rows)?;
    require_shape(&witness.left_inverse, MatrixRole::LeftInverse, rows, rows)?;
    require_shape(&witness.right, MatrixRole::RightTransform, cols, cols)?;
    require_shape(&witness.right_inverse, MatrixRole::RightInverse, cols, cols)?;

    let retained = source
        .entries
        .len()
        .checked_add(witness.diagonal.entries.len())
        .and_then(|value| value.checked_add(witness.left.entries.len()))
        .and_then(|value| value.checked_add(witness.left_inverse.entries.len()))
        .and_then(|value| value.checked_add(witness.right.entries.len()))
        .and_then(|value| value.checked_add(witness.right_inverse.entries.len()))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "retained smith witness entries",
        })?;
    if retained > budget.max_retained_entries {
        return Err(IntegralTopologyError::RetainedEntryBudgetExceeded {
            requested: retained,
            max: budget.max_retained_entries,
        });
    }
    let workspace = rows
        .checked_mul(cols)
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "smith workspace entries",
        })?;
    if workspace > budget.max_workspace_entries {
        return Err(IntegralTopologyError::WorkspaceEntryBudgetExceeded {
            requested: workspace,
            max: budget.max_workspace_entries,
        });
    }
    Ok(())
}

fn require_shape(
    matrix: &ExactIntegerMatrix,
    role: MatrixRole,
    expected_rows: usize,
    expected_cols: usize,
) -> Result<(), IntegralTopologyError> {
    if matrix.rows != expected_rows || matrix.cols != expected_cols {
        return Err(IntegralTopologyError::WitnessShape {
            role,
            expected_rows,
            expected_cols,
            actual_rows: matrix.rows,
            actual_cols: matrix.cols,
        });
    }
    Ok(())
}

fn planned_scalar_operations(rows: usize, cols: usize) -> Result<u128, IntegralTopologyError> {
    let rows = u128::try_from(rows).map_err(|_| IntegralTopologyError::WorkPlanOverflow {
        phase: "row work units",
    })?;
    let cols = u128::try_from(cols).map_err(|_| IntegralTopologyError::WorkPlanOverflow {
        phase: "column work units",
    })?;
    let left_inverse = rows
        .checked_mul(rows)
        .and_then(|value| value.checked_mul(rows))
        .and_then(|value| value.checked_mul(2))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "left inverse verification work",
        })?;
    let right_inverse = cols
        .checked_mul(cols)
        .and_then(|value| value.checked_mul(cols))
        .and_then(|value| value.checked_mul(2))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "right inverse verification work",
        })?;
    let left_times_source = rows
        .checked_mul(rows)
        .and_then(|value| value.checked_mul(cols))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "left transform work",
        })?;
    let diagonal_transform = rows
        .checked_mul(cols)
        .and_then(|value| value.checked_mul(cols))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "right transform work",
        })?;
    left_inverse
        .checked_add(right_inverse)
        .and_then(|value| value.checked_add(left_times_source))
        .and_then(|value| value.checked_add(diagonal_transform))
        .ok_or(IntegralTopologyError::WorkPlanOverflow {
            phase: "total smith verification work",
        })
}

fn verify_identity_product(
    left: &ExactIntegerMatrix,
    right: &ExactIntegerMatrix,
    stage: SmithWitnessStage,
    checkpoint: &mut impl FnMut(&'static str) -> bool,
    completed: &mut u128,
    planned: u128,
) -> Result<(), IntegralTopologyError> {
    for row in 0..left.rows {
        for col in 0..right.cols {
            poll(checkpoint, "smith inverse product", *completed, planned)?;
            let actual = checked_dot(left, row, right, col, stage, completed)?;
            let expected = i128::from(row == col);
            if actual != expected {
                return Err(IntegralTopologyError::WitnessProductMismatch {
                    stage,
                    row,
                    col,
                    expected,
                    actual,
                });
            }
        }
    }
    Ok(())
}

fn checked_dot(
    left: &ExactIntegerMatrix,
    row: usize,
    right: &ExactIntegerMatrix,
    col: usize,
    stage: SmithWitnessStage,
    completed: &mut u128,
) -> Result<i128, IntegralTopologyError> {
    debug_assert_eq!(left.cols, right.rows);
    let mut sum = 0_i128;
    for term in 0..left.cols {
        let product = left
            .entry(row, term)
            .checked_mul(right.entry(term, col))
            .ok_or(IntegralTopologyError::ArithmeticOverflow {
                stage,
                row,
                col,
                term,
            })?;
        sum = sum
            .checked_add(product)
            .ok_or(IntegralTopologyError::ArithmeticOverflow {
                stage,
                row,
                col,
                term,
            })?;
        *completed = completed
            .checked_add(1)
            .ok_or(IntegralTopologyError::WorkPlanOverflow {
                phase: "completed scalar operations",
            })?;
    }
    Ok(sum)
}

fn verify_canonical_diagonal(
    diagonal: &ExactIntegerMatrix,
    checkpoint: &mut impl FnMut(&'static str) -> bool,
    completed: u128,
    planned: u128,
) -> Result<Vec<i128>, IntegralTopologyError> {
    for row in 0..diagonal.rows {
        poll(checkpoint, "smith canonical diagonal", completed, planned)?;
        for col in 0..diagonal.cols {
            if row != col {
                let value = diagonal.entry(row, col);
                if value != 0 {
                    return Err(IntegralTopologyError::OffDiagonalEntry { row, col, value });
                }
            }
        }
    }

    let mut invariant_factors = Vec::new();
    invariant_factors
        .try_reserve_exact(diagonal.rows.min(diagonal.cols))
        .map_err(|_| IntegralTopologyError::AllocationRefused {
            phase: "smith invariant factors",
            requested_entries: diagonal.rows.min(diagonal.cols),
        })?;
    let mut zero_seen = false;
    poll(checkpoint, "smith invariant factors", completed, planned)?;
    for index in 0..diagonal.rows.min(diagonal.cols) {
        let value = diagonal.entry(index, index);
        if value < 0 {
            return Err(IntegralTopologyError::NegativeInvariantFactor { index, value });
        }
        if value == 0 {
            zero_seen = true;
            continue;
        }
        if zero_seen {
            return Err(IntegralTopologyError::NonzeroAfterZero { index, value });
        }
        if let Some(previous) = invariant_factors.last().copied()
            && !value.is_multiple_of(previous)
        {
            return Err(IntegralTopologyError::InvariantFactorDivisibility {
                index,
                previous,
                value,
            });
        }
        invariant_factors.push(value);
    }
    Ok(invariant_factors)
}

fn allocate_zeroed(
    entries: usize,
    phase: &'static str,
) -> Result<Vec<i128>, IntegralTopologyError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(entries)
        .map_err(|_| IntegralTopologyError::AllocationRefused {
            phase,
            requested_entries: entries,
        })?;
    values.resize(entries, 0);
    Ok(values)
}

fn poll(
    checkpoint: &mut impl FnMut(&'static str) -> bool,
    phase: &'static str,
    completed: u128,
    planned: u128,
) -> Result<(), IntegralTopologyError> {
    if checkpoint(phase) {
        Ok(())
    } else {
        Err(IntegralTopologyError::Cancelled {
            phase,
            completed_scalar_operations: completed,
            planned_scalar_operations: planned,
        })
    }
}

/// Structured fail-closed exact-algebra refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegralTopologyError {
    /// The requested phase has no admitted component binding in the pair.
    UnknownTerminalRelativePhase {
        /// Rejected phase identity.
        phase: String,
    },
    /// Boundary degree exceeded the unaugmented chain-complex edge.
    BoundaryDegreeOutOfRange {
        /// Requested source degree.
        degree: u8,
        /// Highest admitted source degree (`complex dimension + 1`).
        max: u8,
    },
    /// An opaque admitted pair lost its retained phase/component invariant.
    PhaseComponentBindingLost {
        /// Phase whose component could not be recovered.
        phase: String,
        /// Component identity retained by the phase map.
        component: String,
    },
    /// Component-support traversal exceeded its explicit envelope.
    ComponentVisitBudgetExceeded {
        /// Required support-cell visits.
        requested: usize,
        /// Maximum admitted visits.
        max: usize,
    },
    /// Ambient incidence traversal exceeded its explicit envelope.
    IncidenceVisitBudgetExceeded {
        /// Required incidence visits.
        requested: usize,
        /// Maximum admitted visits.
        max: usize,
    },
    /// Cancellation was observed while binding a pair boundary.
    PairBoundaryCancelled {
        /// Observation phase.
        phase: &'static str,
        /// Deterministic component/incidence work items completed.
        completed_work_items: u128,
        /// Deterministic component/incidence work items planned.
        planned_work_items: u128,
    },
    /// Adjacent boundaries disagreed on a retained semantic binding.
    AdjacentBoundaryBindingMismatch {
        /// First mismatched binding field.
        field: &'static str,
    },
    /// Candidate boundaries were not consecutive maps `A_k`, `A_(k+1)`.
    AdjacentBoundaryDegreeMismatch {
        /// Outgoing source degree `k`.
        outgoing: u8,
        /// Supplied incoming source degree.
        incoming_source: u8,
        /// Supplied incoming target degree.
        incoming_target: Option<u8>,
    },
    /// Shared `C_k` bases differed in cell identity or order.
    AdjacentBoundaryBasisMismatch {
        /// First differing index.
        index: usize,
        /// Outgoing source-basis cell, if present.
        outgoing: Option<CellRef>,
        /// Incoming target-basis cell, if present.
        incoming: Option<CellRef>,
    },
    /// An opaque verified/boundary value violated an internal shape invariant.
    KernelCoordinateInvariantLost {
        /// Broken invariant field.
        field: &'static str,
    },
    /// The supplied Smith authority described a different source shape.
    OutgoingSmithSourceShapeMismatch {
        /// Required outgoing rows.
        expected_rows: usize,
        /// Required shared-chain columns.
        expected_cols: usize,
        /// Smith-bound source rows.
        actual_rows: usize,
        /// Smith-bound source columns.
        actual_cols: usize,
    },
    /// The supplied Smith authority described different outgoing matrix bytes.
    OutgoingSmithSourceEntryMismatch {
        /// First differing row.
        row: usize,
        /// First differing column.
        col: usize,
        /// Pair-bound outgoing entry.
        expected: i128,
        /// Smith-bound source entry.
        actual: i128,
    },
    /// One adjacent-boundary extent exceeded the transport envelope.
    KernelCoordinateExtentExceeded {
        /// Outgoing target extent `m`.
        outgoing_rows: usize,
        /// Shared chain extent `n`.
        chain_extent: usize,
        /// Incoming source extent `p`.
        incoming_cols: usize,
        /// Maximum extent for each axis.
        max: usize,
    },
    /// Exact source/basis binding comparisons exceeded their envelope.
    KernelBindingBudgetExceeded {
        /// Required comparison items.
        requested: usize,
        /// Maximum admitted items.
        max: usize,
    },
    /// `V^-1 * A_(k+1)` had a nonzero nonkernel coordinate.
    IncomingImageOutsideKernel {
        /// Smith-coordinate row below `rank(A_k)`.
        row: usize,
        /// Incoming basis column.
        col: usize,
        /// Exact nonzero coordinate.
        coordinate: i128,
        /// Positive Smith invariant factor proving this is nonkernel.
        invariant_factor: i128,
    },
    /// Cancellation was observed before kernel-coordinate publication.
    KernelCoordinateCancelled {
        /// Observation phase.
        phase: &'static str,
        /// Completed binding comparisons.
        completed_binding_items: usize,
        /// Planned binding comparisons.
        planned_binding_items: usize,
        /// Completed checked dot-product terms.
        completed_scalar_operations: u128,
        /// Planned checked dot-product terms.
        planned_scalar_operations: u128,
    },
    /// Matrix extent exceeded its explicit envelope.
    MatrixExtentExceeded {
        /// Supplied rows.
        rows: usize,
        /// Supplied columns.
        cols: usize,
        /// Maximum rows.
        max_rows: usize,
        /// Maximum columns.
        max_cols: usize,
    },
    /// `rows * cols` exceeded the per-matrix entry envelope.
    MatrixEntryBudgetExceeded {
        /// Requested entries.
        requested: usize,
        /// Maximum entries.
        max: usize,
    },
    /// Row-major entry count did not match the declared rectangle.
    MatrixEntryCount {
        /// Declared rows.
        rows: usize,
        /// Declared columns.
        cols: usize,
        /// Required entries.
        expected: usize,
        /// Supplied entries.
        actual: usize,
    },
    /// A previously admitted retained matrix exceeds the verification budget.
    RetainedMatrixExceedsBudget {
        /// Matrix role.
        role: MatrixRole,
        /// Retained rows.
        rows: usize,
        /// Retained columns.
        cols: usize,
        /// Retained entries.
        entries: usize,
    },
    /// Complete retained matrix/basis or source/witness storage exceeded the
    /// envelope.
    RetainedEntryBudgetExceeded {
        /// Requested retained entries.
        requested: usize,
        /// Maximum retained entries.
        max: usize,
    },
    /// Verification scratch storage exceeded the envelope.
    WorkspaceEntryBudgetExceeded {
        /// Requested scratch entries.
        requested: usize,
        /// Maximum scratch entries.
        max: usize,
    },
    /// One witness matrix had the wrong exact shape.
    WitnessShape {
        /// Matrix role.
        role: MatrixRole,
        /// Required rows.
        expected_rows: usize,
        /// Required columns.
        expected_cols: usize,
        /// Supplied rows.
        actual_rows: usize,
        /// Supplied columns.
        actual_cols: usize,
    },
    /// Exact scalar work exceeded its admitted cap.
    ScalarWorkBudgetExceeded {
        /// Planned scalar operations.
        requested: u128,
        /// Maximum scalar operations.
        max: u128,
    },
    /// Work accounting overflowed before execution.
    WorkPlanOverflow {
        /// Refusing phase.
        phase: &'static str,
    },
    /// Internal exact matrix, basis, or workspace allocation refused.
    AllocationRefused {
        /// Refusing allocation phase.
        phase: &'static str,
        /// Requested entries.
        requested_entries: usize,
    },
    /// Cancellation was observed before any verified value was published.
    Cancelled {
        /// Observation phase.
        phase: &'static str,
        /// Exact completed scalar operations.
        completed_scalar_operations: u128,
        /// Exact planned scalar operations.
        planned_scalar_operations: u128,
    },
    /// Checked `i128` multiplication or addition overflowed.
    ArithmeticOverflow {
        /// Product being checked.
        stage: SmithWitnessStage,
        /// Output row.
        row: usize,
        /// Output column.
        col: usize,
        /// Inner-product term.
        term: usize,
    },
    /// An inverse or transformed-product witness disagreed exactly.
    WitnessProductMismatch {
        /// Product being checked.
        stage: SmithWitnessStage,
        /// Output row.
        row: usize,
        /// Output column.
        col: usize,
        /// Exact expected integer.
        expected: i128,
        /// Exact observed integer.
        actual: i128,
    },
    /// Claimed Smith matrix contained a nonzero off-diagonal entry.
    OffDiagonalEntry {
        /// Matrix row.
        row: usize,
        /// Matrix column.
        col: usize,
        /// Rejected value.
        value: i128,
    },
    /// Canonical invariant factors cannot be negative.
    NegativeInvariantFactor {
        /// Diagonal index.
        index: usize,
        /// Rejected value.
        value: i128,
    },
    /// A positive invariant appeared after the diagonal's zero suffix began.
    NonzeroAfterZero {
        /// Diagonal index.
        index: usize,
        /// Rejected value.
        value: i128,
    },
    /// Consecutive positive invariant factors violated divisibility.
    InvariantFactorDivisibility {
        /// Later diagonal index.
        index: usize,
        /// Previous factor.
        previous: i128,
        /// Rejected later factor.
        value: i128,
    },
}

impl IntegralTopologyError {
    /// Distinguish a mathematical/structural counterexample from an
    /// inconclusive resource or arithmetic refusal.
    #[must_use]
    pub const fn failure_class(&self) -> IntegralTopologyFailureClass {
        match self {
            Self::UnknownTerminalRelativePhase { .. }
            | Self::BoundaryDegreeOutOfRange { .. }
            | Self::AdjacentBoundaryBindingMismatch { .. }
            | Self::AdjacentBoundaryDegreeMismatch { .. }
            | Self::AdjacentBoundaryBasisMismatch { .. }
            | Self::OutgoingSmithSourceShapeMismatch { .. }
            | Self::OutgoingSmithSourceEntryMismatch { .. }
            | Self::IncomingImageOutsideKernel { .. }
            | Self::MatrixEntryCount { .. }
            | Self::WitnessShape { .. }
            | Self::WitnessProductMismatch { .. }
            | Self::OffDiagonalEntry { .. }
            | Self::NegativeInvariantFactor { .. }
            | Self::NonzeroAfterZero { .. }
            | Self::InvariantFactorDivisibility { .. } => IntegralTopologyFailureClass::Refuted,
            Self::PhaseComponentBindingLost { .. }
            | Self::ComponentVisitBudgetExceeded { .. }
            | Self::IncidenceVisitBudgetExceeded { .. }
            | Self::PairBoundaryCancelled { .. }
            | Self::KernelCoordinateInvariantLost { .. }
            | Self::KernelCoordinateExtentExceeded { .. }
            | Self::KernelBindingBudgetExceeded { .. }
            | Self::KernelCoordinateCancelled { .. }
            | Self::MatrixExtentExceeded { .. }
            | Self::MatrixEntryBudgetExceeded { .. }
            | Self::RetainedMatrixExceedsBudget { .. }
            | Self::RetainedEntryBudgetExceeded { .. }
            | Self::WorkspaceEntryBudgetExceeded { .. }
            | Self::ScalarWorkBudgetExceeded { .. }
            | Self::WorkPlanOverflow { .. }
            | Self::AllocationRefused { .. }
            | Self::Cancelled { .. }
            | Self::ArithmeticOverflow { .. } => IntegralTopologyFailureClass::Unknown,
        }
    }
}

impl fmt::Display for IntegralTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "integral topology admission refused: {self:?}")
    }
}

impl core::error::Error for IntegralTopologyError {}
