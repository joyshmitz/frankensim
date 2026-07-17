# FrankenSim material seed data v1

This directory retains the human-reviewable source inputs compiled by
`xtask matdb-pack`. Generated binary packs are deliberately not committed:
the compiler must reproduce them from the pinned manifest and source record.

## Current species-association tranches

| Directory | Species | NASA molecular weight | Program role |
| --- | --- | ---: | --- |
| `methane/` | `CH4` | `16.04246 g/mol` | fuel constituent |
| `nitrogen/` | `N2` | `28.01340 g/mol` | dry-air/exhaust constituent |
| `oxygen/` | `O2` | `31.99880 g/mol` | dry-air/exhaust constituent |
| `argon/` | `Ar` | `39.94800 g/mol` | dry-air/exhaust constituent |
| `carbon-dioxide/` | `CO2` | `44.00950 g/mol` | exhaust constituent |
| `water-vapor/` | `H2O` | `18.01528 g/mol` | exhaust/humidity constituent |
| `carbon-monoxide/` | `CO` | `28.01010 g/mol` | exhaust constituent |

Each directory seeds one immutable species association with ideal-gas phase
and EOS, reference pressure `100 kPa` (the source report's `1 bar` gas
standard state), and elemental-reference convention
`NASA-TP-2002-211556-reference-elements-298.15K-1bar`.

The primary source is McBride, Zehe, and Gordon, *NASA Glenn Coefficients for
Calculating Thermodynamic Properties of Individual Species*,
NASA/TP-2002-211556 (2002), NTRS document `20020085330`. Appendix B reports
the seven gas molecular weights, while the Standard States section defines the
ideal-gas standard pressure as `1 bar`. The NTRS record marks the report
publicly distributable and as a work of the U.S. Government whose public use
is permitted. These seeds copy only the factual associations above, retain
NASA attribution, and do not copy third-party figures or tables.

As independent spot checks, the NIST Chemistry WebBook SRD 69 pages report
`16.0425` for methane, `28.0134` for nitrogen, `31.9988` for oxygen, `39.948`
for argon, `44.0095` for carbon dioxide, `18.0153` for water, and `28.0101`
for carbon monoxide. Each displayed value agrees with the corresponding NASA
value within one half-unit at NIST's displayed precision. NIST values are
comparison oracles only; they are not pack sources and do not replace the
retained NASA values.

Primary and comparison references:

- <https://ntrs.nasa.gov/citations/20020085330>
- <https://ntrs.nasa.gov/api/citations/20020085330/downloads/20020085330.pdf>
- <https://sti.nasa.gov/disclaimers/>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C74828>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C7727379>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C7782447>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C7440371>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C124389>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C7732185>
- <https://webbook.nist.gov/cgi/cbook.cgi?ID=C630080>

## Aluminum 6061-T6 cryogenic exact-point tranche

`aluminum-6061-t6-cryogenic/` is the first committed bulk-material tranche.
It identifies the condition as Aluminum 6061-T6, UNS AA96061, and retains
thermal conductivity, specific heat capacity, and Young's modulus at exactly
`77 K` and `293 K`. Each value is an evaluation of the polynomial printed on
NIST's Aluminum 6061-T6 material-property page, using the page's displayed
coefficients and equation. The runtime claims have degenerate temperature
validity intervals (`T_min = T_max`), so they do not imply interpolation or a
continuous curve.

NIST reports curve-fit errors relative to the underlying data of `0.5%` for
thermal conductivity, `5%` for specific heat, and `1%` for Young's modulus.
Those figures do not state a confidence level or degrees of freedom, so the
source records retain them as observation caveats and explicitly encode the
runtime uncertainty as `Unstated`. They are not laundered into statistical
confidence intervals.

As independent G3 comparison evidence, the 1966 NASA thermophysical-property
compilation tabulates 6061-T6 thermal conductivity as `82 W/(m K)` at `75 K`
and `155 W/(m K)` at `300 K`. Those nearby-temperature values differ from the
NIST-derived `77 K` and `293 K` points by less than `3%`; they are comparison
oracles only and do not overwrite the NIST-derived claims. NASA's NTRS record
for its 1969 Aluminum 6061 handbook independently confirms that the handbook
covers cryogenic, ambient, and elevated-temperature properties, but this
tranche does not silently import values from that separate work.

The NIST page is not marked as copyrighted. NIST's Copyrights & Disclaimers
page says unmarked NIST-site information is public information that may be
distributed or copied and requests appropriate credit. The manifest records
that specific redistribution decision and retains NIST attribution; it does
not generalize the decision to separately licensed NIST research products.

Material references:

- <https://www.nist.gov/mml/acmd/aluminum-6061-t6-uns-aa96061>
- <https://www.nist.gov/copyrights-disclaimers>
- <https://ntrs.nasa.gov/api/citations/19660014513/downloads/19660014513.pdf>
- <https://ntrs.nasa.gov/citations/19690000065>

## OFHC Copper cryogenic exact-point tranche

`ofhc-copper-rrr100/` retains NIST's combined OFHC Copper scope, UNS
C10100/C10200, without pretending that the two UNS designations are
interchangeable for properties the source does not cover. Thermal-conductivity
claims pin `RRR = 100`, a load-bearing condition because NIST publishes
distinct conductivity correlations for RRR 50, 100, 150, 300, and 500. The
specific-heat source publishes one combined OFHC correlation without an RRR
condition, so its observation names that omission explicitly instead of
silently borrowing the conductivity condition.

The tranche evaluates the published RRR-100 conductivity correlation and
specific-heat polynomial at exactly `77 K` and `293 K`. As with the Aluminum
tranche, degenerate validity intervals prevent interpolation claims. NIST
reports `1%` curve-fit error for the RRR-100 conductivity correlation and `5%`
for specific heat at these temperatures, but does not give confidence levels
or degrees of freedom. The pack therefore retains the errors and complete
coefficient sets as observation caveats while encoding `Unstated` statistical
uncertainty.

NASA-CR-134806 independently reports typical room-temperature OFHC Copper
specific heat of `386 J/(kg K)` and thermal conductivity of `390 W/(m K)`.
Both are within `2%` of the NIST-derived `293 K` values. That coarse G3 check
is evidence only: it neither overwrites the NIST claims nor erases NASA's
distinct material/processing context. The NTRS record marks the NASA report
public and as a work of the U.S. Government whose public use is permitted.

OFHC Copper references:

- <https://trc.nist.gov/cryogenics/materials/OFHC%20Copper/OFHC_Copper_rev1.htm>
- <https://www.nist.gov/publications/properties-copper-and-copper-alloys-cryogenic-temperatures>
- <https://ntrs.nasa.gov/citations/19750021165>
- <https://ntrs.nasa.gov/api/citations/19750021165/downloads/19750021165.pdf>

## AISI 4140 Rockwell C33 low-temperature tranche

`aisi-4140-rc33/` is a condition-pinned structural-steel tranche from
NASA-TM-X-64791, not a generic 4140 card. The retained specimen is QQ-S-624
heat `137M186`, machined from nominal one-inch bar. NASA normalized the stock
at `899 degC`, hardened it at `851 degC`, oil quenched it, tempered it at
`566 degC`, and measured Rockwell C33. Those processing and hardness fields
are part of every observation identity.

At the report's exact `26.7 degC` and `-73 degC` test points, the pack retains
ultimate tensile strength, 0.2%-offset yield strength, two-inch tensile
elongation, reduction of area, MIL-STD-151 Charpy V-notch impact energy, and
ultimate and yield double-shear strengths. The NASA table reports averages
from five tensile specimen sets, four impact tests at each retained point, and
four shear specimens. It does not publish dispersion or confidence metadata,
so every runtime uncertainty is explicitly `Unstated`; sample counts are
observation caveats, not fabricated error bars.

The G3 pack battery checks NASA's redundant ksi/GPa columns for the retained
tensile values within their printed rounding precision. This is a source-unit
transcription check, not an independent-source agreement claim. No second
source with a sufficiently identical heat, geometry, process schedule,
hardness, and temperature condition is treated as interchangeable.

AISI 4140 references:

- <https://ntrs.nasa.gov/citations/19740002417>
- <https://ntrs.nasa.gov/api/citations/19740002417/downloads/19740002417.pdf>

## AISI 1045 cold-drawn tensile tranche

`aisi-1045-cold-drawn/` retains one experimental tensile series from Kang and
Lee's 2024 turning study, not a generic handbook card for AISI 1045. The source
identifies an approximately `0.45 wt% C` cold-drawn AISI 1045 bar with `37 mm`
outside diameter and `102 mm` starting length. Three ASTM E8 cylindrical
specimens used a `50 mm` gauge length, `12.5 mm` test-section diameter,
extensometry, and `10 mm/min` crosshead speed.

The pack retains the paper's reported means: `550.51 MPa` yield strength,
`695.31 MPa` ultimate tensile strength, and `14.1%` elongation over the
`50 mm` gauge length. The source also prints all three replicate values for
each property. The pack derives symmetric 95% Student-t half-widths for the
mean from those replicates using `t(0.975, df=2) = 4.302652729911275` under an
explicit iid-normal assumption: `21.7281028138069 MPa`,
`29.0129099774989 MPa`, and `0.943972330500626` percentage points,
respectively. These are small-sample confidence intervals for the reported
series, not grade tolerances or certified material allowables. Although the
three properties were measured on paired specimen rows, the pack does not
invent a joint covariance block.

The paper does not report tensile-test temperature. Every claim therefore
requires both the exact normalized crosshead-speed point and the explicit
dimensionless context `source_test_temperature_known = 0`. That second axis is
a fail-closed acknowledgement of missing source metadata, not a physical
temperature. No `temperature` validity interval is present, and a caller must
not interpret these values as temperature-independent. The paper is published
under Creative Commons Attribution 4.0 International; the manifest retains the
authors, title, journal, DOI, and license.

The paper's printed Vickers-hardness replicates round to a mean different from
the mean printed in the same table, so this tranche refuses to select either
number. No second source with the same bar, cold-drawn state, specimen geometry,
test rate, and unreported temperature is treated as interchangeable.

AISI 1045 reference:

- <https://www.mdpi.com/2227-9717/12/6/1171>

To compile the sources into canonical runtime packs:

```bash
cargo run -p xtask -- matdb-pack \
  --manifest data/matdb/seed-v1/methane/manifest.tsv \
  --out /path/to/CH4.fsspcpk

cargo run -p xtask -- matdb-pack \
  --manifest data/matdb/seed-v1/aluminum-6061-t6-cryogenic/manifest.tsv \
  --out /path/to/aluminum-6061-t6-cryogenic.fsmatpk

cargo run -p xtask -- matdb-pack \
  --manifest data/matdb/seed-v1/ofhc-copper-rrr100/manifest.tsv \
  --out /path/to/ofhc-copper-cryogenic.fsmatpk

cargo run -p xtask -- matdb-pack \
  --manifest data/matdb/seed-v1/aisi-4140-rc33/manifest.tsv \
  --out /path/to/aisi-4140-qq-s-624-rc33.fsmatpk

cargo run -p xtask -- matdb-pack \
  --manifest data/matdb/seed-v1/aisi-1045-cold-drawn/manifest.tsv \
  --out /path/to/aisi-1045-cold-drawn-tensile.fsmatpk
```

## No-claim boundary

The gas tranches are species identity/standard-state associations, not complete
material cards. They do not specify an air or exhaust mixture, composition
basis, humidity level, combustion state, or validity domain. In particular,
the H2O association does not select a wet-air composition and the CO
association does not claim incomplete combustion. They supply no heat-capacity
coefficients, equation evaluator, uncertainty model for thermodynamic
properties, reaction mechanism, equilibrium result, or transport data. A
decimal agreement check is not an uncertainty estimate.

The Aluminum 6061-T6 tranche is not a general-purpose design card. It contains
six polynomial-derived scalar points for one named temper, not density,
composition tolerances, anisotropy, batch/process variation, plasticity,
strength, fatigue, fracture, corrosion, joining, or a continuous-temperature
constitutive model. NIST's curve-fit error is retained but not represented as a
confidence interval. The NASA comparison uses nearby temperatures and is only
a coarse independent agreement check. These missing claims and the remaining
named materials and interface systems keep bead
`frankensim-ext-matdb-seed-dataset-1sxe` open.

The OFHC Copper tranche likewise does not select between UNS C10100 and C10200,
define electrical resistivity, density, mechanical strength, temper, grain
state, purity beyond the source's OFHC class, or a continuous-temperature
model. Only its conductivity claims bind `RRR = 100`; the source leaves RRR
unstated for specific heat, and the stored observation preserves that gap.

The AISI 4140 tranche applies only to the exact NASA heat/process/hardness/bar
condition above and the two retained temperatures. It does not provide a
continuous-temperature law, elastic modulus or Poisson ratio, a constitutive
plasticity model, fatigue or fracture curves, general cryogenic qualification,
or permission to substitute another 4140 heat treatment. The report's Rockwell
C44 branch remains a separate conflicting condition and is not fused here.

The AISI 1045 tranche applies only to the source's cold-drawn bar and three
ASTM E8 specimens at the retained test rate. The source omits test temperature,
supplier, heat number, and any heat-treatment history beyond `cold-drawn`;
callers must explicitly acknowledge the missing temperature metadata through
the fail-closed validity flag. The derived intervals assume three iid normal
replicates and are not population scatter, minimum design values, process
capability, or permission to substitute a different AISI 1045 condition. The
tranche makes no hardness claim and admits no joint covariance.
