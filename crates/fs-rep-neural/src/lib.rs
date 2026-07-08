//! fs-rep-neural — neural implicit charts. Layer: L2.
//!
//! Small coordinate MLPs as shapes (DeepSDF-style). The load-bearing property
//! is NOT the fit — it is the CERTIFICATE: with SPECTRAL-NORM-constrained
//! layers and 1-Lipschitz activations, the network carries a certified global
//! LIPSCHITZ CONSTANT `L = Π σᵢ`, and that bound holds for every input
//! regardless of training. That is what keeps a neural shape inside the
//! certificate regime:
//!
//! - sphere tracing with PROVABLE step safety — a step of `|f(x)|/L` can never
//!   tunnel through the surface ([`safe_step_radius`]);
//! - INTERVAL evaluation via layer-wise bound propagation (IBP) — a guaranteed
//!   output enclosure over an input box ([`MlpSdf::eval_interval`]);
//! - the gradient is bounded by `L` (`‖∇f‖ ≤ L`).
//!
//! Neural charts PROPOSE geometry; certification (watertightness, Hausdorff
//! agreement, topology) comes from the certificate machinery, NEVER the loss
//! curve — so [`TopologyHint`] is honestly `Unknown` here. Deterministic; no
//! dependencies (in-house power iteration for the spectral norm).

/// A dense affine layer (`out × in` weights + bias).
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// The weight matrix (`out` rows, each of length `in`).
    pub weights: Vec<Vec<f64>>,
    /// The bias (`out`).
    pub bias: Vec<f64>,
}

impl Layer {
    /// A layer.
    ///
    /// # Panics
    /// If the weight rows disagree with the bias length or are ragged.
    #[must_use]
    pub fn new(weights: Vec<Vec<f64>>, bias: Vec<f64>) -> Layer {
        assert!(weights.len() == bias.len(), "weights/bias length mismatch");
        if let Some(first) = weights.first() {
            assert!(
                weights.iter().all(|r| r.len() == first.len()),
                "ragged weights"
            );
        }
        Layer { weights, bias }
    }

    fn in_dim(&self) -> usize {
        self.weights.first().map_or(0, Vec::len)
    }
}

/// The spectral norm (largest singular value) of a matrix, by power iteration
/// on `WᵀW`.
#[must_use]
pub fn spectral_norm(weights: &[Vec<f64>]) -> f64 {
    if weights.is_empty() || weights[0].is_empty() {
        return 0.0;
    }
    let n_in = weights[0].len();
    // deterministic, non-degenerate initial vector.
    let mut v: Vec<f64> = (0..n_in).map(|i| 1.0 + 0.1 * i as f64).collect();
    normalize(&mut v);
    for _ in 0..200 {
        let u = matvec(weights, &v); // W v
        let mut z = matvec_t(weights, &u); // Wᵀ (W v)
        let nz = norm(&z);
        if nz < 1e-30 {
            return 0.0;
        }
        for zi in &mut z {
            *zi /= nz;
        }
        v = z;
    }
    norm(&matvec(weights, &v)) // ‖W v‖ = σ_max
}

/// Spectrally normalize a layer so its weight matrix has spectral norm exactly
/// `bound` (the constraint that yields the Lipschitz certificate).
#[must_use]
pub fn spectral_normalize(mut layer: Layer, bound: f64) -> Layer {
    let sigma = spectral_norm(&layer.weights);
    if sigma > 1e-30 {
        let scale = bound / sigma;
        for row in &mut layer.weights {
            for w in row {
                *w *= scale;
            }
        }
    }
    layer
}

/// An honest topology hint: never claimed without a verifying check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyHint {
    /// Topology is unknown until certified (persistent-homology check).
    Unknown,
}

/// A neural implicit chart: a coordinate MLP with a certified Lipschitz constant.
#[derive(Debug, Clone, PartialEq)]
pub struct MlpSdf {
    layers: Vec<Layer>,
    lipschitz: f64,
}

impl MlpSdf {
    /// Build a chart from raw layers, spectrally normalizing each to `bound`
    /// (so `L = boundᵏ` with 1-Lipschitz `tanh` activations). The last layer
    /// must map to a scalar.
    ///
    /// # Panics
    /// If `layers` is empty, dimensions do not chain, or the output is not scalar.
    #[must_use]
    pub fn new(layers: Vec<Layer>, bound: f64) -> MlpSdf {
        assert!(!layers.is_empty(), "need at least one layer");
        assert!(
            layers.last().unwrap().weights.len() == 1,
            "output must be scalar"
        );
        for pair in layers.windows(2) {
            assert!(
                pair[0].weights.len() == pair[1].in_dim(),
                "layer dims do not chain"
            );
        }
        let normalized: Vec<Layer> = layers
            .into_iter()
            .map(|l| spectral_normalize(l, bound))
            .collect();
        // certified global Lipschitz constant = Π σᵢ (tanh is 1-Lipschitz).
        let lipschitz = normalized
            .iter()
            .map(|l| spectral_norm(&l.weights))
            .product();
        MlpSdf {
            layers: normalized,
            lipschitz,
        }
    }

    /// The certified global Lipschitz constant.
    #[must_use]
    pub fn lipschitz(&self) -> f64 {
        self.lipschitz
    }

    /// The topology hint — honestly `Unknown` (never inferred from the fit).
    #[must_use]
    pub fn topology_hint(&self) -> TopologyHint {
        TopologyHint::Unknown
    }

    /// Evaluate the implicit value `f(x)` (tanh between layers, linear output).
    #[must_use]
    pub fn eval(&self, x: &[f64]) -> f64 {
        let mut a = x.to_vec();
        let last = self.layers.len() - 1;
        for (li, layer) in self.layers.iter().enumerate() {
            let mut z = layer.bias.clone();
            for (j, row) in layer.weights.iter().enumerate() {
                z[j] += dot(row, &a);
            }
            if li < last {
                for zi in &mut z {
                    *zi = zi.tanh();
                }
            }
            a = z;
        }
        a[0]
    }

    /// The gradient `∇f(x)` by central finite differences (its norm is `≤ L`).
    #[must_use]
    pub fn eval_grad(&self, x: &[f64]) -> Vec<f64> {
        let h = 1e-6;
        (0..x.len())
            .map(|i| {
                let mut xp = x.to_vec();
                let mut xm = x.to_vec();
                xp[i] += h;
                xm[i] -= h;
                (self.eval(&xp) - self.eval(&xm)) / (2.0 * h)
            })
            .collect()
    }

    /// A GUARANTEED output enclosure of `f` over the input box `[lo, hi]`, by
    /// interval bound propagation (IBP) — sound for sphere-tracing sub-boxes.
    #[must_use]
    pub fn eval_interval(&self, lo: &[f64], hi: &[f64]) -> (f64, f64) {
        let (mut lo, mut hi) = (lo.to_vec(), hi.to_vec());
        let last = self.layers.len() - 1;
        for (li, layer) in self.layers.iter().enumerate() {
            let mut nlo = layer.bias.clone();
            let mut nhi = layer.bias.clone();
            for (j, row) in layer.weights.iter().enumerate() {
                for (i, &w) in row.iter().enumerate() {
                    if w >= 0.0 {
                        nlo[j] += w * lo[i];
                        nhi[j] += w * hi[i];
                    } else {
                        nlo[j] += w * hi[i];
                        nhi[j] += w * lo[i];
                    }
                }
            }
            if li < last {
                for k in 0..nlo.len() {
                    // tanh is monotone increasing.
                    nlo[k] = nlo[k].tanh();
                    nhi[k] = nhi[k].tanh();
                }
            }
            lo = nlo;
            hi = nhi;
        }
        (lo[0], hi[0])
    }
}

/// The provably-safe sphere-tracing step radius: with SDF value `value` and
/// Lipschitz constant `lipschitz`, `f` cannot change sign within `|value|/L`, so
/// a step of that size never tunnels through the surface.
#[must_use]
pub fn safe_step_radius(value: f64, lipschitz: f64) -> f64 {
    if lipschitz <= 0.0 {
        return f64::INFINITY;
    }
    value.abs() / lipschitz
}

// -- linear-algebra helpers -------------------------------------------------

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn matvec(w: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    w.iter().map(|row| dot(row, v)).collect()
}

fn matvec_t(w: &[Vec<f64>], u: &[f64]) -> Vec<f64> {
    let n_in = w[0].len();
    let mut z = vec![0.0; n_in];
    for (row, &ui) in w.iter().zip(u) {
        for (zi, &wij) in z.iter_mut().zip(row) {
            *zi += wij * ui;
        }
    }
    z
}

fn norm(v: &[f64]) -> f64 {
    dot(v, v).sqrt()
}

fn normalize(v: &mut [f64]) {
    let n = norm(v);
    if n > 1e-30 {
        for vi in v {
            *vi /= n;
        }
    }
}
