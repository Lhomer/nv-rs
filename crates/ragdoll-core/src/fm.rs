//! The same maths on every machine: the transcendental functions the physics uses, from the
//! portable `libm` crate instead of the system's maths library.
//!
//! `f32::sin` and the like call the system's library, which picks its code by the processor
//! (glibc chooses FMA versions where the processor has them), so their last bits differ from
//! machine to machine; the simulation is chaotic and grows that into another fall (the Ragdoll
//! lab's stand read the centre 0.07 m outside the feet in CI and -0.03 m here, from the same
//! code). `sqrt`, `powi` and the four operations are exact or software and need no stand-in.

/// The physics' transcendental functions, the same bits everywhere (`d` for deterministic).
pub trait Fm: Sized {
    fn dsin(self) -> Self;
    fn dcos(self) -> Self;
    fn dtan(self) -> Self;
    fn dasin(self) -> Self;
    fn dacos(self) -> Self;
    fn datan2(self, x: Self) -> Self;
    fn dexp(self) -> Self;
    fn dpowf(self, e: Self) -> Self;
    fn dtanh(self) -> Self;
    fn dsin_cos(self) -> (Self, Self) {
        let s = Self::dsin_raw(&self);
        (s, Self::dcos_raw(&self))
    }
    #[doc(hidden)]
    fn dsin_raw(x: &Self) -> Self;
    #[doc(hidden)]
    fn dcos_raw(x: &Self) -> Self;
}

impl Fm for f32 {
    fn dsin(self) -> f32 {
        libm::sinf(self)
    }
    fn dcos(self) -> f32 {
        libm::cosf(self)
    }
    fn dtan(self) -> f32 {
        libm::tanf(self)
    }
    fn dasin(self) -> f32 {
        libm::asinf(self)
    }
    fn dacos(self) -> f32 {
        libm::acosf(self)
    }
    fn datan2(self, x: f32) -> f32 {
        libm::atan2f(self, x)
    }
    fn dexp(self) -> f32 {
        libm::expf(self)
    }
    fn dpowf(self, e: f32) -> f32 {
        libm::powf(self, e)
    }
    fn dtanh(self) -> f32 {
        libm::tanhf(self)
    }
    fn dsin_raw(x: &f32) -> f32 {
        libm::sinf(*x)
    }
    fn dcos_raw(x: &f32) -> f32 {
        libm::cosf(*x)
    }
}

impl Fm for f64 {
    fn dsin(self) -> f64 {
        libm::sin(self)
    }
    fn dcos(self) -> f64 {
        libm::cos(self)
    }
    fn dtan(self) -> f64 {
        libm::tan(self)
    }
    fn dasin(self) -> f64 {
        libm::asin(self)
    }
    fn dacos(self) -> f64 {
        libm::acos(self)
    }
    fn datan2(self, x: f64) -> f64 {
        libm::atan2(self, x)
    }
    fn dexp(self) -> f64 {
        libm::exp(self)
    }
    fn dpowf(self, e: f64) -> f64 {
        libm::pow(self, e)
    }
    fn dtanh(self) -> f64 {
        libm::tanh(self)
    }
    fn dsin_raw(x: &f64) -> f64 {
        libm::sin(*x)
    }
    fn dcos_raw(x: &f64) -> f64 {
        libm::cos(*x)
    }
}

#[cfg(test)]
mod tests {
    use super::Fm;

    /// The stand-ins answer as the functions they stand in for, to the last few bits.
    #[test]
    fn they_answer_as_the_standard_functions() {
        for &x in &[-2.5f32, -1.0, -0.3, 0.0, 0.2, 0.9, 1.7, 3.0] {
            let close = |a: f32, b: f32| (a - b).abs() <= 4.0 * f32::EPSILON * b.abs().max(1.0);
            assert!(close(x.dsin(), x.sin()) && close(x.dcos(), x.cos()), "{x}");
            assert!(
                close(x.dexp(), x.exp()) && close(x.dtanh(), x.tanh()),
                "{x}"
            );
            assert!(close(x.datan2(0.7), x.atan2(0.7)), "{x}");
            let u = x / 3.0;
            assert!(
                close(u.dacos(), u.acos()) && close(u.dasin(), u.asin()),
                "{x}"
            );
            assert!(close(x.abs().dpowf(1.3), x.abs().powf(1.3)), "{x}");
            let (s, c) = x.dsin_cos();
            assert!(close(s, x.sin()) && close(c, x.cos()), "{x}");
        }
    }
}
