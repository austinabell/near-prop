# near-prop

Property-based testing for NEAR using [workspaces-rs](https://github.com/near/workspaces-rs).

The APIs are an extension of [quickcheck](https://github.com/BurntSushi/quickcheck) which is used for generating data and shrinking data to find minimal failures.

## Usage

```rust
    #[near_prop::test]
    async fn prop_test_basic(ctx: PropContext, amount: u64) -> anyhow::Result<bool> {
        let r = ctx.contract
            .call(&ctx.worker, "add")
            .args_json((amount,))?
            .transact()
            .await?
            .json::<u64>()?;
        Ok(r == amount)
    }
```

This will by default compile the current directory's library and run until 100 tests pass. These tests will be run in parallel.

See more examples [here](https://github.com/austinabell/near-prop/tree/main/examples/)

Future functionality:
- [ ] Allow overriding wasm file or cargo manifest directory
- [ ] Allow re-using contract with prop tests
- [ ] Integration with contract ABI (automatic fuzzing of methods?)
