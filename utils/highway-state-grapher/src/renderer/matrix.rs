use std::ops;

#[derive(Clone, Copy)]
pub struct Matrix {
    coords: [[f32; 4]; 4],
}

impl Matrix {
    pub fn identity() -> Matrix {
        Matrix {
            coords: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn inner(self) -> [[f32; 4]; 4] {
        self.coords
    }

    pub fn translation(x: f32, y: f32) -> Matrix {
        let mut result = Matrix::identity();
        result.coords[3][0] = x;
        result.coords[3][1] = y;
        result
    }

    pub fn scale(x: f32, y: f32) -> Matrix {
        Matrix {
            coords: [
                [x, 0.0, 0.0, 0.0],
                [0.0, y, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }
}

impl ops::Add<Matrix> for Matrix {
    type Output = Matrix;

    fn add(mut self, other: Matrix) -> Matrix {
        for i in 0..4 {
            for j in 0..4 {
                self.coords[i][j] += other.coords[i][j];
            }
        }
        self
    }
}

impl ops::Sub<Matrix> for Matrix {
    type Output = Matrix;

    fn sub(mut self, other: Matrix) -> Matrix {
        for i in 0..4 {
            for j in 0..4 {
                self.coords[i][j] -= other.coords[i][j];
            }
        }
        self
    }
}

impl ops::Mul<Matrix> for Matrix {
    type Output = Matrix;

    #[allow(clippy::needless_range_loop)]
    fn mul(self, other: Matrix) -> Matrix {
        let mut new_coords = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    new_coords[i][j] += self.coords[i][k] * other.coords[k][j];
                }
            }
        }
        Matrix { coords: new_coords }
    }
}
