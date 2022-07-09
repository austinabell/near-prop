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
        self.count += amount;
        self.count
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use near_prop::{TestResult, NearProp};
    use workspaces::prelude::*;

    #[tokio::test]
    async fn prop_test_add() -> anyhow::Result<()> {
        async fn prop(amount: u64) -> anyhow::Result<TestResult> {
            let wasm = workspaces::compile_project("./").await?;
            let worker = workspaces::sandbox().await?;
            let contract = worker.dev_deploy(&wasm).await?;
            let r = contract
                .call(&worker, "add")
                .args_json((amount,))?
                .transact()
                .await?
                .json::<u64>()?;
            Ok(TestResult::from_bool(r == amount))
        }
        NearProp::default().tests(20).test(prop as fn(_) -> _).await;
        // prop_test(prop as fn(_) -> _).await;
        Ok(())
    }
}
