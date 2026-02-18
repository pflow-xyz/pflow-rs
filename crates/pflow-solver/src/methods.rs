//! Runge-Kutta solver methods (Butcher tableaux).

/// An ODE solver method defined by its Butcher tableau.
#[derive(Debug, Clone)]
pub struct Solver {
    pub name: &'static str,
    pub order: usize,
    pub c: Vec<f64>,
    pub a: Vec<Vec<f64>>,
    pub b: Vec<f64>,
    pub b_hat: Vec<f64>,
}

/// Tsitouras 5/4 Runge-Kutta solver.
///
/// A 5th order explicit method with embedded 4th order error estimator.
pub fn tsit5() -> Solver {
    Solver {
        name: "Tsit5",
        order: 5,
        c: vec![0.0, 0.161, 0.327, 0.9, 0.9800255409045097, 1.0, 1.0],
        a: vec![
            vec![],
            vec![0.161],
            vec![-0.008480655492356924, 0.335480655492357],
            vec![2.8971530571054935, -6.359448489975075, 4.362295432869581],
            vec![
                5.325864828439257,
                -11.748883564062828,
                7.4955393428898365,
                -0.09249506636175525,
            ],
            vec![
                5.86145544294642,
                -12.92096931784711,
                8.159367898576159,
                -0.071584973281401,
                -0.028269050394068383,
            ],
            vec![
                0.09646076681806523,
                0.01,
                0.4798896504144996,
                1.379008574103742,
                -3.290069515436081,
                2.324710524099774,
                0.0,
            ],
        ],
        b: vec![
            0.09646076681806523,
            0.01,
            0.4798896504144996,
            1.379008574103742,
            -3.290069515436081,
            2.324710524099774,
            0.0,
        ],
        b_hat: vec![
            0.001780011052226,
            0.000816434459657,
            -0.007880878010262,
            0.144711007173263,
            -0.582357165452555,
            0.458082105929187,
            1.0 / 66.0,
        ],
    }
}

/// Dormand-Prince 5(4) Runge-Kutta solver (MATLAB's ode45).
pub fn rk45() -> Solver {
    Solver {
        name: "RK45",
        order: 5,
        c: vec![0.0, 1.0 / 5.0, 3.0 / 10.0, 4.0 / 5.0, 8.0 / 9.0, 1.0, 1.0],
        a: vec![
            vec![],
            vec![1.0 / 5.0],
            vec![3.0 / 40.0, 9.0 / 40.0],
            vec![44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0],
            vec![
                19372.0 / 6561.0,
                -25360.0 / 2187.0,
                64448.0 / 6561.0,
                -212.0 / 729.0,
            ],
            vec![
                9017.0 / 3168.0,
                -355.0 / 33.0,
                46732.0 / 5247.0,
                49.0 / 176.0,
                -5103.0 / 18656.0,
            ],
            vec![
                35.0 / 384.0,
                0.0,
                500.0 / 1113.0,
                125.0 / 192.0,
                -2187.0 / 6784.0,
                11.0 / 84.0,
            ],
        ],
        b: vec![
            35.0 / 384.0,
            0.0,
            500.0 / 1113.0,
            125.0 / 192.0,
            -2187.0 / 6784.0,
            11.0 / 84.0,
            0.0,
        ],
        b_hat: vec![
            35.0 / 384.0 - 5179.0 / 57600.0,
            0.0,
            500.0 / 1113.0 - 7571.0 / 16695.0,
            125.0 / 192.0 - 393.0 / 640.0,
            -2187.0 / 6784.0 + 92097.0 / 339200.0,
            11.0 / 84.0 - 187.0 / 2100.0,
            -1.0 / 40.0,
        ],
    }
}

/// Classic 4th order Runge-Kutta (fixed step).
pub fn rk4() -> Solver {
    Solver {
        name: "RK4",
        order: 4,
        c: vec![0.0, 0.5, 0.5, 1.0],
        a: vec![vec![], vec![0.5], vec![0.0, 0.5], vec![0.0, 0.0, 1.0]],
        b: vec![1.0 / 6.0, 1.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0],
        b_hat: vec![0.0, 0.0, 0.0, 0.0],
    }
}

/// Forward Euler method (1st order, fixed step).
pub fn euler() -> Solver {
    Solver {
        name: "Euler",
        order: 1,
        c: vec![0.0],
        a: vec![vec![]],
        b: vec![1.0],
        b_hat: vec![0.0],
    }
}

/// Heun's method / improved Euler (2nd order).
pub fn heun() -> Solver {
    Solver {
        name: "Heun",
        order: 2,
        c: vec![0.0, 1.0],
        a: vec![vec![], vec![1.0]],
        b: vec![0.5, 0.5],
        b_hat: vec![0.0, 0.0],
    }
}

/// Midpoint method (2nd order).
pub fn midpoint() -> Solver {
    Solver {
        name: "Midpoint",
        order: 2,
        c: vec![0.0, 0.5],
        a: vec![vec![], vec![0.5]],
        b: vec![0.0, 1.0],
        b_hat: vec![0.0, 0.0],
    }
}

/// Bogacki-Shampine 3(2) method.
pub fn bs32() -> Solver {
    Solver {
        name: "BS32",
        order: 3,
        c: vec![0.0, 0.5, 0.75, 1.0],
        a: vec![
            vec![],
            vec![0.5],
            vec![0.0, 0.75],
            vec![2.0 / 9.0, 1.0 / 3.0, 4.0 / 9.0],
        ],
        b: vec![2.0 / 9.0, 1.0 / 3.0, 4.0 / 9.0, 0.0],
        b_hat: vec![
            2.0 / 9.0 - 7.0 / 24.0,
            1.0 / 3.0 - 1.0 / 4.0,
            4.0 / 9.0 - 1.0 / 3.0,
            -1.0 / 8.0,
        ],
    }
}
