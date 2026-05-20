//! Mock runtime for `pallet-midds` tests.
//!
//! Hosts a single `pallet_midds` instance (default `I = ()`) over a trivial
//! [`MockMidds`] payload, just enough to exercise the lifecycle mechanics
//! (deposit, update, remove_own, finalize, force_*) without depending on the
//! real `MusicalWork` payload. The default-instance choice is required by
//! `impl_benchmark_test_suite!`, which wires `Pallet::<Test>` (i.e. `I = ()`).

use crate as pallet_midds;
use frame_support::{
    BoundedVec, derive_impl,
    traits::{ConstU32, ConstU64},
};
use frame_system::EnsureRoot;
use midds_traits::MiddsFormatError;
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{
    BuildStorage, FixedU128,
    traits::{Get, IdentifyAccount, Verify},
};

pub type AccountId = u64;
pub type Balance = u64;
pub type BlockNumber = u64;
pub type Block = frame_system::mocking::MockBlock<Test>;

#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub struct TestSigner(pub u64);

impl IdentifyAccount for TestSigner {
    type AccountId = u64;
    fn into_account(self) -> u64 {
        self.0
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub struct TestSignature {
    pub signer: u64,
    pub payload: Vec<u8>,
}

impl Verify for TestSignature {
    type Signer = TestSigner;
    fn verify<L: sp_runtime::traits::Lazy<[u8]>>(&self, mut msg: L, signer: &u64) -> bool {
        self.signer == *signer && self.payload == msg.get()
    }
}

/// Build a mock signature for `signer` over the SCALE encoding of `payload`.
pub fn sign(signer: u64, payload: &impl Encode) -> TestSignature {
    TestSignature {
        signer,
        payload: payload.encode(),
    }
}

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Balances: pallet_balances,
        Midds: pallet_midds,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = pallet_balances::AccountData<Balance>;
    type AccountId = AccountId;
    type Lookup = sp_runtime::traits::IdentityLookup<AccountId>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
    type Balance = Balance;
    type AccountStore = System;
    type ExistentialDeposit = ConstU64<1>;
    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeFreezeReason;
}

/// Identifier of a [`MockMidds`]: an ASCII bounded string up to 8 bytes.
pub type MockId = BoundedVec<u8, ConstU32<8>>;
/// Free-form payload bytes for a [`MockMidds`].
pub type MockPayload = BoundedVec<u8, ConstU32<32>>;

/// Trivial MIDDS payload used only by the mock runtime tests.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct MockMidds {
    pub id: MockId,
    pub data: MockPayload,
}

impl midds_traits::Midds for MockMidds {
    const KIND: &'static str = "MockMidds";

    type Identifier = MockId;

    fn identifier(&self) -> &Self::Identifier {
        &self.id
    }

    fn validate_format(&self) -> Result<(), MiddsFormatError> {
        if self.id.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        if !self.id.iter().all(u8::is_ascii_alphanumeric) {
            return Err(MiddsFormatError::InvalidCharset);
        }
        Ok(())
    }
}

/// Mock helper saturating at the [`MockMidds`] payload bound (32 bytes) and
/// folding `size` into the canonical id so distinct sizes yield distinct
/// payloads — the contract `BenchmarkHelper` users (e.g. `force_remove_many`)
/// rely on now that `PayloadHashes` rejects byte-identical re-deposits.
#[cfg(feature = "runtime-benchmarks")]
pub struct MockBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_midds::BenchmarkHelper<MockMidds, TestSignature, AccountId> for MockBenchmarkHelper {
    fn bench_instance(size: u32) -> MockMidds {
        let payload_len = (size as usize).min(32);
        let data: Vec<u8> = (0..payload_len).map(|_| size as u8).collect();
        MockMidds {
            id: BoundedVec::try_from(b"bench".to_vec()).expect("`bench` fits the 8-byte bound"),
            data: BoundedVec::try_from(data).expect("clamped to the 32-byte bound"),
        }
    }

    fn create_signature(entropy: &[u8], msg: &[u8]) -> (TestSignature, AccountId) {
        let mut buf = [0u8; 8];
        for (i, b) in entropy.iter().take(8).enumerate() {
            buf[i] = *b;
        }
        let signer: AccountId = u64::from_le_bytes(buf).saturating_add(10_000);
        (
            TestSignature {
                signer,
                payload: msg.to_vec(),
            },
            signer,
        )
    }
}

/// Flat part of the bond formula for the mock runtime.
pub const DEPOSIT_BASE: Balance = 10;
/// Per-byte multiplier of the bond formula for the mock runtime.
pub const DEPOSIT_PER_BYTE: Balance = 1;
/// Length of the commitment window for the mock runtime, in blocks.
pub const COMMITMENT_WINDOW: BlockNumber = 100;
/// Mock "day" length — 10 blocks. `SLOW_WINDOW = 7 × BlocksPerDay`. Picked
/// short enough that integration tests exercising slow-window rotation stay
/// fast yet long enough that off-by-one tests don't accidentally cross a
/// rotation boundary.
pub const BLOCKS_PER_DAY: BlockNumber = 10;
/// Soft cap so a stuffed deposit-burst single test bucket can't drag M_fast
/// to its max in one block — leaves headroom for adjacent cases that depend
/// on observable, predictable multiplier movement.
pub const FAST_TARGET_PER_BLOCK: u32 = 5;
pub const SLOW_TARGET_PER_WINDOW: u32 = 200;
/// Cap on records cleaned in a single `force_remove_many` invocation. The
/// benchmark sweeps `Linear<1, 64>` so this must be ≥ 64 for the mock
/// runtime that hosts `impl_benchmark_test_suite!`.
pub const MAX_REMOVALS_PER_CALL: u32 = 64;
/// Foundation Treasury account in the mock — picked outside the genesis-
/// endowed range so its starting balance is the existential deposit (none).
pub const TREASURY: AccountId = 999;

pub struct TreasuryAccount;
impl Get<AccountId> for TreasuryAccount {
    fn get() -> AccountId {
        TREASURY
    }
}

pub struct FastAdjustmentRate;
impl Get<FixedU128> for FastAdjustmentRate {
    fn get() -> FixedU128 {
        FixedU128::from_rational(125, 1_000)
    }
}

pub struct SlowAdjustmentRate;
impl Get<FixedU128> for SlowAdjustmentRate {
    fn get() -> FixedU128 {
        FixedU128::from_rational(5, 100)
    }
}

pub struct MultiplierMin;
impl Get<FixedU128> for MultiplierMin {
    fn get() -> FixedU128 {
        FixedU128::from_rational(1, 10)
    }
}

pub struct FastMultiplierMax;
impl Get<FixedU128> for FastMultiplierMax {
    fn get() -> FixedU128 {
        FixedU128::from_u32(20)
    }
}

pub struct SlowMultiplierMax;
impl Get<FixedU128> for SlowMultiplierMax {
    fn get() -> FixedU128 {
        FixedU128::from_u32(50)
    }
}

impl pallet_midds::Config for Test {
    type Currency = Balances;
    type RuntimeHoldReason = RuntimeHoldReason;
    type Midds = MockMidds;
    type ProviderOrigin = frame_system::EnsureSigned<AccountId>;
    type ForceOrigin = EnsureRoot<AccountId>;
    type OffchainSignature = TestSignature;
    type Signer = TestSigner;
    type TreasuryAccount = TreasuryAccount;
    type DepositBase = ConstU64<DEPOSIT_BASE>;
    type DepositPerByte = ConstU64<DEPOSIT_PER_BYTE>;
    type CommitmentWindow = ConstU64<COMMITMENT_WINDOW>;
    type MaxFinalizationsPerBlock = ConstU32<100>;
    type MaxRemovalsPerCall = ConstU32<MAX_REMOVALS_PER_CALL>;
    type BlocksPerDay = ConstU64<BLOCKS_PER_DAY>;
    type FastTargetPerBlock = ConstU32<FAST_TARGET_PER_BLOCK>;
    type FastAdjustmentRate = FastAdjustmentRate;
    type FastMultiplierMin = MultiplierMin;
    type FastMultiplierMax = FastMultiplierMax;
    type SlowTargetPerWindow = ConstU32<SLOW_TARGET_PER_WINDOW>;
    type SlowAdjustmentRate = SlowAdjustmentRate;
    type SlowMultiplierMin = MultiplierMin;
    type SlowMultiplierMax = SlowMultiplierMax;
    type WeightInfo = ();
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = MockBenchmarkHelper;
}

/// Build genesis storage with three pre-funded accounts and start at block 1.
pub fn new_test_ext() -> sp_io::TestExternalities {
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("frame_system genesis build");
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![(1, 1_000_000), (2, 1_000_000), (3, 1_000_000)],
        ..Default::default()
    }
    .assimilate_storage(&mut t)
    .expect("balances genesis build");
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

/// Build a [`MockMidds`] with the provided id bytes and a fixed-size payload.
pub fn mock_midds(id: &[u8], payload_len: usize) -> MockMidds {
    MockMidds {
        id: BoundedVec::try_from(id.to_vec()).expect("id within bound"),
        data: BoundedVec::try_from(vec![0u8; payload_len]).expect("data within bound"),
    }
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use frame_support::traits::fungible::{Inspect, InspectHold};
    use parity_scale_codec::Encode;

    pub const ALICE: AccountId = 1;
    pub const BOB: AccountId = 2;
    pub const CHARLIE: AccountId = 3;

    pub fn deposit_reason() -> RuntimeHoldReason {
        crate::HoldReason::<()>::Deposit.into()
    }

    /// Unmultiplied base bond — what `remove_own` refunds and what
    /// `mass_injection` cross-checks.
    pub fn expected_base_bond_for(item: &MockMidds) -> Balance {
        let size = item.encoded_size() as Balance;
        DEPOSIT_BASE + DEPOSIT_PER_BYTE * size
    }

    /// Bond at the *current* multipliers (read live from storage) — the
    /// amount actually held on a successful `deposit` at this block.
    pub fn expected_total_bond_for(item: &MockMidds) -> Balance {
        crate::Pallet::<Test, ()>::current_deposit_price(item.encoded_size() as u32)
    }

    pub fn held(account: AccountId) -> Balance {
        <Balances as InspectHold<AccountId>>::balance_on_hold(&deposit_reason(), &account)
    }

    pub fn free(account: AccountId) -> Balance {
        <Balances as Inspect<AccountId>>::reducible_balance(
            &account,
            frame_support::traits::tokens::Preservation::Expendable,
            frame_support::traits::tokens::Fortitude::Polite,
        )
    }

    pub fn treasury_balance() -> Balance {
        <Balances as Inspect<AccountId>>::balance(&TREASURY)
    }

    /// Run the pallet hook for the next block(s), mirroring the production
    /// `Executive` flow. Useful in tests that need to observe finalization
    /// and multiplier rotation effects.
    pub fn advance_to_block(target: BlockNumber) {
        use frame_support::traits::Hooks;
        let mut now = System::block_number();
        while now < target {
            now += 1;
            System::set_block_number(now);
            <crate::Pallet<Test, ()> as Hooks<BlockNumber>>::on_initialize(now);
        }
    }
}
