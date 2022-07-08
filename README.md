# near-prop

Property-based testing for NEAR using [workspaces-rs](https://github.com/near/workspaces-rs).

API goal

```rust
prop(async |contract, worker, f: FuzzData /* params? */| {
  let result = contract.call(worker, "fuzzme").args_json(f)?.transact().await?;
  Ok(())
}).await;
```

Questions:
- How should project be compiled (default to current and override with a path to wasm file?)
- Stateful or stateless transactions?
  - Probably stateful, but might be nice to express clearing state or re-deploying after x txs
- Allow fuzzing random inputs
  - Could possibly take all function names and fuzz them with random data
- How to handle transfers going over capacity of dev accounts?
- How to handle multi-tx tests?
