use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::near_bindgen;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, Default)]
pub struct StatusMessage {
    count: u64,
}

#[near_bindgen]
impl StatusMessage {
    pub fn add(&mut self, amount: u64) -> u64 {
        // Switch these statements for the prop test to pick up the bug
        // self.count += amount;
        self.count = self.count.saturating_add(amount);
        self.count
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use near_prop::{prop_test, PropContext};
    use workspaces::prelude::*;

    #[near_prop::test]
    async fn prop_test_macro(ctx: PropContext, a: (u64, u64, u64, u64)) -> anyhow::Result<()> {
        let mut acc: u64 = 0;
        // Quickcheck arbitrary doesn't support arrays, so this is a hack around this.
        for amount in [a.0, a.1, a.2, a.3] {
            let r = ctx.contract
                .call(&ctx.worker, "add")
                .args_json((amount,))?
                .transact()
                .await?
                .json::<u64>()?;
            acc = acc.saturating_add(amount);
            if acc != r {
                anyhow::bail!("Invalid value returned, expected {} got {}", acc, r);
            }
        }
        Ok(())
    }

    #[ignore]
    #[tokio::test]
    async fn prop_test_add() {
        async fn prop(ctx: PropContext, a: (u64, u64, u64, u64)) -> anyhow::Result<()> {
            let mut acc: u64 = 0;
            // Quickcheck arbitrary doesn't support arrays, so this is a hack around this.
            for amount in [a.0, a.1, a.2, a.3] {
                let r = ctx.contract
                    .call(&ctx.worker, "add")
                    .args_json((amount,))?
                    .transact()
                    .await?
                    .json::<u64>()?;
                acc = acc.saturating_add(amount);
                if acc != r {
                    anyhow::bail!("Invalid value returned, expected {} got {}", acc, r);
                }
            }
            
            Ok(())
        }
        prop_test(prop as fn(PropContext, _) -> _).await;
    }

    #[tokio::test]
    async fn ws() -> anyhow::Result<()> {
        let wasm = workspaces::compile_project("./").await?;
        let worker = workspaces::sandbox().await?;
        let contract = worker.dev_deploy(&wasm).await?;
        let mut acc: u64 = 0;
        for amount in [9, 100, 987_198_987, 563_324_354_295] {
            let r = contract
                .call(&worker, "add")
                .args_json((amount,))?
                .transact()
                .await?
                .json::<u64>()?;
            acc = acc.saturating_add(amount);
            if acc != r {
                anyhow::bail!("Invalid value returned, expected {} got {}", acc, r);
            }
        }
            
        Ok(())
    }
}
