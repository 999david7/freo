//! Live-Simulation solvers: voxel finite elements (statics, heat, modes) and lattice
//! Boltzmann flow, plus the glue that runs a study of the part on a worker thread.

pub mod fea;
pub mod fluid;
pub mod mesh;
pub mod run;
