/// Set up the panic hook for better error messages in WASM
pub fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    {
        console_error_panic_hook::set_once();
    }
}

/// 3x3 Matrix for 3D transformations
#[derive(Debug, Clone, Copy)]
pub struct Matrix3x3(pub [f64; 9]);

impl Matrix3x3 {
    pub fn identity() -> Self {
        Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
    }

    pub fn multiply(&self, other: &Self) -> Self {
        let mut result = [0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    result[i * 3 + j] += self.0[i * 3 + k] * other.0[k * 3 + j];
                }
            }
        }
        Self(result)
    }

    pub fn transpose(&self) -> Self {
        let m = &self.0;
        Self([
            m[0], m[3], m[6],
            m[1], m[4], m[7],
            m[2], m[5], m[8],
        ])
    }

    pub fn rotation_x(angle_rad: f64) -> Self {
        let c = angle_rad.cos();
        let s = angle_rad.sin();
        Self([
            1.0, 0.0, 0.0,
            0.0, c,   -s,
            0.0, s,   c,
        ])
    }

    pub fn rotation_y(angle_rad: f64) -> Self {
        let c = angle_rad.cos();
        let s = angle_rad.sin();
        Self([
            c,   0.0, s,
            0.0, 1.0, 0.0,
            -s,  0.0, c,
        ])
    }

    pub fn rotation_z(angle_rad: f64) -> Self {
        let c = angle_rad.cos();
        let s = angle_rad.sin();
        Self([
            c,   -s,  0.0,
            s,   c,   0.0,
            0.0, 0.0, 1.0,
        ])
    }

    pub fn transform(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        let m = &self.0;
        (
            m[0] * x + m[1] * y + m[2] * z,
            m[3] * x + m[4] * y + m[5] * z,
            m[6] * x + m[7] * y + m[8] * z,
        )
    }
}
