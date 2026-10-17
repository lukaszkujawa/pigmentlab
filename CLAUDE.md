# PigmentLab - Painting Simulator

Hyper-realistic, real-time painting simulator built in Rust with GPU acceleration.

## Coding Guidelines

- Never comment code. Names and structure must be self-explanatory.
- Prioritise clean, simple, modular design.
- Keep files small and responsibilities focused.
- Prefer idiomatic Rust and established libraries.
- Prioritise performance without premature optimisation.
- Keep CPU/GPU transfers and allocations minimal.
- Avoid unnecessary abstractions, complexity and dependencies.
- Be concise in code, explanations and communication.
- Never suppress errors, warnings or failing tests.

## Testing

- Design for testability from the start.
- Unit test mathematical calculations, physics and simulation logic.
- Keep unit tests fast and deterministic.
- Support automated, reproducible end-to-end scenarios.
- Allow tests to script user actions: select tools, change parameters, press/release buttons, move the pointer and draw strokes.
- Support fixed random seeds and simulation timesteps for reproducibility.
- Capture screenshots at defined checkpoints and compare against reference images using configurable percentage-based differences and tolerances.
- Measure execution time for individual actions, simulation steps, rendering and complete scenarios.
- Detect and report visual, functional and performance regressions.
- Keep regression baselines versioned and review changes explicitly.
- Ask before running long or expensive tests.
- The watercolour GPU suite (`crates/core/tests/watercolour_gpu.rs`) is `#[ignore]`d because it takes ~6 minutes. Run it with `cargo test --release -p pigmentlab-core --test watercolour_gpu -- --ignored` whenever a change touches watercolour code or code it shares (engine stepping, brush simulation, canvas, pigments, shared shaders).
