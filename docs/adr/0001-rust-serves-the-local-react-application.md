# Rust serves the local React application

The production service is started with `cargo run -- serve` and serves both the Rust HTTP API and built React client from one local-service origin. A separate React development server may proxy API requests during UI work, but it is not part of normal use; this keeps the single-user product easy to start without sacrificing frontend iteration speed.
