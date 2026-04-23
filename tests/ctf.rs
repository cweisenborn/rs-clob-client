#![cfg(feature = "ctf")]
#![allow(clippy::unwrap_used, reason = "Fine for tests")]

use alloy::primitives::{B256, U256};
use alloy::providers::ProviderBuilder;
use httpmock::{Method::POST, MockServer};
use polymarket_client_sdk::POLYGON;
use polymarket_client_sdk::ctf::Client;
use polymarket_client_sdk::types::address;
use serde_json::json;

mod contract_calls {
    use alloy::primitives::b256;
    use polymarket_client_sdk::ctf::types::{
        CollectionIdRequest, ConditionIdRequest, PositionIdRequest,
    };

    use super::*;

    #[tokio::test]
    async fn get_condition_id() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;
        let client = Client::new(provider, POLYGON)?;

        // Mock the eth_call JSON-RPC response
        let mock = server.mock(|when, then| {
            when.method(POST).path("/");
            then.json_body(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
            }));
        });

        let request = ConditionIdRequest::builder()
            .oracle(address!("0x0000000000000000000000000000000000000001"))
            .question_id(B256::ZERO)
            .outcome_slot_count(U256::from(2))
            .build();

        let response = client.condition_id(&request).await?;

        // Verify we got the mocked response
        assert_eq!(
            response.condition_id,
            b256!("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        );
        mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn get_collection_id() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;
        let client = Client::new(provider, POLYGON)?;

        let mock = server.mock(|when, then| {
            when.method(POST).path("/");
            then.json_body(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"
            }));
        });

        let request = CollectionIdRequest::builder()
            .parent_collection_id(B256::ZERO)
            .condition_id(B256::ZERO)
            .index_set(U256::from(1))
            .build();

        let response = client.collection_id(&request).await?;

        // Verify we got the mocked response
        assert_eq!(
            response.collection_id,
            b256!("abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd")
        );
        mock.assert();

        Ok(())
    }

    #[tokio::test]
    async fn get_position_id() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;
        let client = Client::new(provider, POLYGON)?;

        let mock = server.mock(|when, then| {
            when.method(POST).path("/");
            then.json_body(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x00000000000000000000000000000000000000000000000000000000000000ff"
            }));
        });

        let usdc = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");

        let request = PositionIdRequest::builder()
            .collateral_token(usdc)
            .collection_id(B256::ZERO)
            .build();

        let response = client.position_id(&request).await?;

        // Verify we got the mocked response
        assert_eq!(response.position_id, U256::from(0xff));
        mock.assert();

        Ok(())
    }
}

mod client_creation {
    use super::*;

    #[tokio::test]
    async fn polygon_mainnet_should_succeed() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let client = Client::new(provider, POLYGON);
        client.unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn amoy_testnet_should_succeed() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let client = Client::new(provider, polymarket_client_sdk::AMOY);
        client.unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn invalid_chain_should_fail() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let client = Client::new(provider, 999);
        client.unwrap_err();

        Ok(())
    }
}

mod request_builders {
    use polymarket_client_sdk::ctf::types::{
        ConditionIdRequest, MergePositionsRequest, RedeemPositionsRequest, SplitPositionRequest,
    };

    use super::*;

    #[test]
    fn condition_id_request_builder() {
        let request = ConditionIdRequest::builder()
            .oracle(address!("0x0000000000000000000000000000000000000001"))
            .question_id(B256::ZERO)
            .outcome_slot_count(U256::from(2))
            .build();

        assert_eq!(
            request.oracle,
            address!("0x0000000000000000000000000000000000000001")
        );
        assert_eq!(request.question_id, B256::ZERO);
        assert_eq!(request.outcome_slot_count, U256::from(2));
    }

    #[test]
    fn split_position_request_builder() {
        let request = SplitPositionRequest::builder()
            .collateral_token(address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"))
            .condition_id(B256::ZERO)
            .partition(vec![U256::from(1), U256::from(2)])
            .amount(U256::from(1_000_000))
            .build();

        assert_eq!(
            request.collateral_token,
            address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174")
        );
        assert_eq!(request.parent_collection_id, B256::ZERO);
        assert_eq!(request.amount, U256::from(1_000_000));
    }

    #[test]
    fn merge_positions_request_builder() {
        let request = MergePositionsRequest::builder()
            .collateral_token(address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"))
            .condition_id(B256::ZERO)
            .partition(vec![U256::from(1), U256::from(2)])
            .amount(U256::from(1_000_000))
            .build();

        assert_eq!(request.parent_collection_id, B256::ZERO);
        assert_eq!(request.amount, U256::from(1_000_000));
    }

    #[test]
    fn redeem_positions_request_builder() {
        let request = RedeemPositionsRequest::builder()
            .collateral_token(address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"))
            .condition_id(B256::ZERO)
            .index_sets(vec![U256::from(1)])
            .build();

        assert_eq!(request.parent_collection_id, B256::ZERO);
        assert_eq!(request.index_sets, vec![U256::from(1)]);
    }
}

mod binary_market_convenience_methods {
    use polymarket_client_sdk::ctf::types::{
        MergePositionsRequest, RedeemPositionsRequest, SplitPositionRequest,
    };

    use super::*;

    #[test]
    fn split_position_for_binary_market() {
        let usdc = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
        let condition_id = B256::ZERO;

        let request =
            SplitPositionRequest::for_binary_market(usdc, condition_id, U256::from(1_000_000));

        assert_eq!(request.collateral_token, usdc);
        assert_eq!(request.condition_id, condition_id);
        assert_eq!(request.partition, vec![U256::from(1), U256::from(2)]);
        assert_eq!(request.amount, U256::from(1_000_000));
        assert_eq!(request.parent_collection_id, B256::ZERO);
    }

    #[test]
    fn merge_positions_for_binary_market() {
        let usdc = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
        let condition_id = B256::ZERO;

        let request =
            MergePositionsRequest::for_binary_market(usdc, condition_id, U256::from(1_000_000));

        assert_eq!(request.collateral_token, usdc);
        assert_eq!(request.condition_id, condition_id);
        assert_eq!(request.partition, vec![U256::from(1), U256::from(2)]);
        assert_eq!(request.amount, U256::from(1_000_000));
    }

    #[test]
    fn redeem_positions_for_binary_market() {
        let usdc = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");
        let condition_id = B256::ZERO;

        let request = RedeemPositionsRequest::for_binary_market(usdc, condition_id);

        assert_eq!(request.collateral_token, usdc);
        assert_eq!(request.condition_id, condition_id);
        assert_eq!(request.index_sets, vec![U256::from(1), U256::from(2)]);
    }
}

mod neg_risk {
    use polymarket_client_sdk::ctf::types::RedeemNegRiskRequest;

    use super::*;

    #[test]
    fn redeem_neg_risk_request_builder() {
        let condition_id = B256::ZERO;
        let amounts = vec![U256::from(500_000), U256::from(500_000)];

        let request = RedeemNegRiskRequest::builder()
            .condition_id(condition_id)
            .amounts(amounts.clone())
            .build();

        assert_eq!(request.condition_id, condition_id);
        assert_eq!(request.amounts, amounts);
    }

    #[tokio::test]
    async fn client_with_neg_risk_should_succeed() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let client = Client::with_neg_risk(provider, POLYGON);
        client.unwrap();

        Ok(())
    }

    #[tokio::test]
    async fn with_neg_risk_v2_chain_137_ok() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let result = Client::with_neg_risk_v2(provider, 137);
        assert!(result.is_ok(), "chain 137 has V2 neg-risk adapter configured");

        Ok(())
    }

    #[tokio::test]
    async fn with_neg_risk_v2_chain_80002_errors() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let result = Client::with_neg_risk_v2(provider, 80002);
        assert!(
            result.is_err(),
            "chain 80002 has no V2 neg-risk adapter (neg_risk_ctf_collateral_adapter is None)"
        );

        Ok(())
    }

    #[tokio::test]
    async fn with_neg_risk_v2_chain_unknown_errors() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let result = Client::with_neg_risk_v2(provider, 999_999);
        assert!(result.is_err(), "unknown chain ID has no configuration");

        Ok(())
    }

    #[tokio::test]
    async fn with_standard_v2_chain_137_ok() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let result = Client::with_standard_v2(provider, 137);
        assert!(result.is_ok(), "chain 137 has V2 standard CTF collateral adapter configured");

        Ok(())
    }

    #[tokio::test]
    async fn with_standard_v2_chain_80002_errors() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let result = Client::with_standard_v2(provider, 80002);
        assert!(
            result.is_err(),
            "chain 80002 has no V2 standard CTF collateral adapter (ctf_collateral_adapter is None)"
        );

        Ok(())
    }

    #[tokio::test]
    async fn with_standard_v2_chain_unknown_errors() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        let result = Client::with_standard_v2(provider, 999_999);
        assert!(result.is_err(), "unknown chain ID has no configuration");

        Ok(())
    }

    /// Pins the post-fix invariant for `with_neg_risk_v2`: the client is
    /// shape-compatible with `with_standard_v2` — both bind the
    /// `IConditionalTokens` interface to their respective V2 wrapper addresses
    /// and leave the V1 2-arg `neg_risk_adapter` slot empty. Callers use the
    /// 5-arg `split_position` / `merge_positions` / `redeem_positions` with
    /// pUSD collateral; the 2-arg `split_position_neg_risk` path is V1-only
    /// under this fix (the V2 wrapper does not expose the 2-arg selector —
    /// diagnosed on-chain 2026-04-23 via `eth_call`).
    ///
    /// This test pins the constructor contract via the public Ok/Err surface
    /// (the doc comment on `with_neg_risk_v2` is the structural contract —
    /// private fields are not asserted directly to avoid coupling tests to
    /// implementation layout).
    #[tokio::test]
    async fn with_neg_risk_v2_uses_iconditionaltokens_shape() -> anyhow::Result<()> {
        let server = MockServer::start();
        let provider = ProviderBuilder::new().connect(&server.base_url()).await?;

        // Chain 137 has both V2 wrapper adapters configured.
        let neg_risk = Client::with_neg_risk_v2(provider.clone(), 137);
        assert!(
            neg_risk.is_ok(),
            "chain 137 has V2 neg-risk CTF collateral adapter configured"
        );

        // Sibling V2 constructor — same shape, different wrapper.
        let standard = Client::with_standard_v2(provider, 137);
        assert!(
            standard.is_ok(),
            "chain 137 has V2 standard CTF collateral adapter configured"
        );

        // Both V2 clients are `Client<P>` of the same type — callers use the
        // same 5-arg split_position/merge_positions/redeem_positions methods
        // on each, differing only in which wrapper address backs `contract`.
        let _: &Client<_> = neg_risk.as_ref().unwrap();
        let _: &Client<_> = standard.as_ref().unwrap();

        Ok(())
    }
}
