//! Mass-injection tests — Couche 3 of `docs/testing.md`.
//!
//! Stresses the pallet at N=1k–100k records on an in-test mock runtime wired
//! to the real `MusicalWork` payload (not `MockMidds`) so that the committed
//! storage-root fixture genuinely pins the on-chain SCALE encoding.
//!
//! Each scenario writes `target/test-reports/mass_injection_<scenario>.json`
//! and compares a deterministic projected hash of the pallet storage against
//! `tests/fixtures/storage_root_<scenario>.txt`. When the bond formula or the
//! `MusicalWork` encoding changes, this layer is the first to fail and forces
//! a deliberate fixture refresh:
//!
//! ```sh
//! BLESS_MASS_INJECTION_FIXTURES=1 cargo test -p pallet-midds --test mass_injection
//! ```
//!
//! Heavy scenarios (50k, 100k) are `#[ignore]`d to keep the per-PR Couche 3
//! budget at <5 min (see `docs/testing.md` §11). Run them locally / in
//! nightly with `cargo test -p pallet-midds --test mass_injection -- --ignored`.

use std::{fmt::Write as _, fs, path::PathBuf, time::Instant};

use frame_support::{
    derive_impl,
    traits::{ConstU32, ConstU64, fungible::InspectHold},
};
use frame_system::EnsureRoot;
use midds_fixtures::{gen_n, identifiers::iswc_for_index, pathological::max_size_musical_work};
use midds_types::MusicalWork;
use parity_scale_codec::Encode;
use sp_io::hashing::blake2_256;
use sp_runtime::{BuildStorage, FixedU128, traits::Get};

// -----------------------------------------------------------------------------
// Mock runtime
// -----------------------------------------------------------------------------

type AccountId = u64;
type Balance = u128;
type BlockNumber = u64;
type Block = frame_system::mocking::MockBlock<Test>;

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
    type ExistentialDeposit = ConstU128<1>;
    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeFreezeReason;
}

const DEPOSIT_BASE: Balance = 10;
const DEPOSIT_PER_BYTE: Balance = 1;
const COMMITMENT_WINDOW: BlockNumber = 100;
const TREASURY: AccountId = 999;

pub struct TreasuryAccount;
impl Get<AccountId> for TreasuryAccount {
    fn get() -> AccountId {
        TREASURY
    }
}

pub struct ConstU128<const V: Balance>;
impl<const V: Balance> Get<Balance> for ConstU128<V> {
    fn get() -> Balance {
        V
    }
}

pub struct UnitFixed;
impl Get<FixedU128> for UnitFixed {
    fn get() -> FixedU128 {
        FixedU128::from_u32(1)
    }
}

pub struct MaxFixed;
impl Get<FixedU128> for MaxFixed {
    fn get() -> FixedU128 {
        FixedU128::from_u32(50)
    }
}

pub struct MinFixed;
impl Get<FixedU128> for MinFixed {
    fn get() -> FixedU128 {
        FixedU128::from_rational(1, 10)
    }
}

#[cfg(feature = "runtime-benchmarks")]
pub struct MusicalWorkBenchHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_midds::BenchmarkHelper<MusicalWork> for MusicalWorkBenchHelper {
    fn bench_instance(_size: u32) -> MusicalWork {
        midds_fixtures::pathological::min_size_musical_work()
    }
}

impl pallet_midds::Config for Test {
    type Currency = Balances;
    type RuntimeHoldReason = RuntimeHoldReason;
    type Midds = MusicalWork;
    type ProviderOrigin = frame_system::EnsureSigned<AccountId>;
    type ForceOrigin = EnsureRoot<AccountId>;
    type TreasuryAccount = TreasuryAccount;
    type DepositBase = ConstU128<DEPOSIT_BASE>;
    type DepositPerByte = ConstU128<DEPOSIT_PER_BYTE>;
    type CommitmentWindow = ConstU64<COMMITMENT_WINDOW>;
    type MaxFinalizationsPerBlock = ConstU32<100>;
    type BlocksPerDay = ConstU64<10>;
    type FastTargetPerBlock = ConstU32<1_000_000>;
    type FastAdjustmentRate = UnitFixed;
    type FastMultiplierMin = MinFixed;
    type FastMultiplierMax = MaxFixed;
    type SlowTargetPerWindow = ConstU32<1_000_000_000>;
    type SlowAdjustmentRate = UnitFixed;
    type SlowMultiplierMin = MinFixed;
    type SlowMultiplierMax = MaxFixed;
    type WeightInfo = ();
    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper = MusicalWorkBenchHelper;
}

// -----------------------------------------------------------------------------
// Externalities builder
// -----------------------------------------------------------------------------

fn build_ext(account_count: u64, balance_each: Balance) -> sp_io::TestExternalities {
    let mut t = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .expect("frame_system genesis");
    let balances: Vec<(AccountId, Balance)> =
        (1..=account_count).map(|i| (i, balance_each)).collect();
    pallet_balances::GenesisConfig::<Test> {
        balances,
        ..Default::default()
    }
    .assimilate_storage(&mut t)
    .expect("balances genesis");
    let mut ext = sp_io::TestExternalities::new(t);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

// -----------------------------------------------------------------------------
// Scenario harness
// -----------------------------------------------------------------------------

struct ScenarioStats {
    scenario: &'static str,
    n: u64,
    total_bond_held: u128,
    avg_encoded_size_bytes: u64,
    storage_root_hash: String,
    wall_time_ms: u128,
}

fn run_scenario(scenario: &'static str, items: Vec<MusicalWork>, depositors: &[AccountId]) {
    assert!(!depositors.is_empty(), "depositors slice must not be empty");
    let n = items.len();

    // Targets so high the multipliers never adjust away from 1.0× over the
    // lifetime of the run — ensures `total_bond == base_bond` and lets the
    // stats harness keep its analytical mirror of `do_compute_base_bond`.
    let total_required_bond: Balance = items
        .iter()
        .map(|i| DEPOSIT_BASE + DEPOSIT_PER_BYTE * i.encoded_size() as Balance)
        .sum::<Balance>();
    let per_account = total_required_bond / depositors.len() as Balance;
    let balance_each = per_account.saturating_mul(2).saturating_add(1_000_000);
    let max_account = *depositors.iter().max().expect("non-empty");

    let mut ext = build_ext(max_account, balance_each);

    let started = Instant::now();
    let (sum_size, sum_bond) = ext.execute_with(|| {
        let mut sum_size: u128 = 0;
        let mut sum_bond: u128 = 0;
        for (i, item) in items.iter().enumerate() {
            let depositor = depositors[i % depositors.len()];
            let size = item.encoded_size();
            sum_size += size as u128;
            sum_bond += DEPOSIT_BASE + DEPOSIT_PER_BYTE * size as Balance;
            pallet_midds::Pallet::<Test>::deposit(RuntimeOrigin::signed(depositor), item.clone())
                .unwrap_or_else(|e| panic!("deposit #{i} failed: {e:?}"));
        }
        (sum_size, sum_bond)
    });
    let wall_time_ms = started.elapsed().as_millis();

    let on_chain_bond = ext.execute_with(|| {
        let mut total: u128 = 0;
        for &d in depositors {
            total += <Balances as InspectHold<AccountId>>::balance_on_hold(
                &pallet_midds::HoldReason::<()>::Deposit.into(),
                &d,
            );
        }
        total
    });
    assert_eq!(
        on_chain_bond, sum_bond,
        "Σ held(account) ≠ Σ DepositBase + DepositPerByte * size"
    );

    let storage_hash = ext.execute_with(projected_storage_hash);
    let stats = ScenarioStats {
        scenario,
        n: n as u64,
        total_bond_held: sum_bond,
        avg_encoded_size_bytes: (sum_size / n as u128) as u64,
        storage_root_hash: format!("0x{}", hex(&storage_hash)),
        wall_time_ms,
    };

    write_report(&stats);
    check_or_bless(scenario, &stats.storage_root_hash);
}

// -----------------------------------------------------------------------------
// Storage projection
// -----------------------------------------------------------------------------

/// Deterministic hash of the pallet's storage state.
///
/// Concatenates SCALE-encoded `(key, value)` pairs in a fixed order
/// (`NextMiddsId`, then `(id, item)` and `(id, deposit_info)` for each id in
/// `0..NextMiddsId`, then the multi-claim `(identifier, id)` pairs sorted
/// canonically) and BLAKE2b-256 hashes the resulting bytes.
fn projected_storage_hash() -> [u8; 32] {
    let mut buf: Vec<u8> = Vec::new();

    let next = pallet_midds::NextMiddsId::<Test, ()>::get();
    buf.extend(next.encode());

    for id in 0..next {
        if let Some(item) = pallet_midds::Items::<Test, ()>::get(id) {
            buf.extend(id.encode());
            buf.extend(item.encode());
        }
        if let Some(info) = pallet_midds::DepositInfo::<Test, ()>::get(id) {
            buf.extend(id.encode());
            buf.extend(info.encode());
        }
    }

    let mut claims: Vec<(midds_traits::Iswc, midds_traits::MiddsId)> =
        pallet_midds::IdentifierClaims::<Test, ()>::iter()
            .map(|(ident, id, _)| (ident, id))
            .collect();
    claims.sort();
    for (ident, id) in claims {
        buf.extend(ident.encode());
        buf.extend(id.encode());
    }

    blake2_256(&buf)
}

// -----------------------------------------------------------------------------
// Reports / fixtures I/O
// -----------------------------------------------------------------------------

fn report_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("test-reports")
}

fn fixture_path(scenario: &str) -> PathBuf {
    let short = scenario.strip_prefix("mass_injection_").unwrap_or(scenario);
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(format!("storage_root_{short}.txt"))
}

fn write_report(stats: &ScenarioStats) {
    let dir = report_dir();
    fs::create_dir_all(&dir).expect("create target/test-reports");
    let json = serde_json::json!({
        "scenario": stats.scenario,
        "n": stats.n,
        "total_bond_held": stats.total_bond_held,
        "avg_encoded_size_bytes": stats.avg_encoded_size_bytes,
        "storage_root_hash": stats.storage_root_hash,
        "wall_time_ms": stats.wall_time_ms as u64,
        "peak_memory_kb": serde_json::Value::Null,
    });
    let path = dir.join(format!("{}.json", stats.scenario));
    fs::write(
        &path,
        serde_json::to_string_pretty(&json).expect("serialize report"),
    )
    .expect("write report");
}

fn check_or_bless(scenario: &str, hash: &str) {
    let path = fixture_path(scenario);
    if std::env::var_os("BLESS_MASS_INJECTION_FIXTURES").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixtures dir");
        }
        fs::write(&path, hash).expect("write fixture");
        eprintln!("blessed {} -> {hash}", path.display());
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing fixture {}: {err}. Run with \
             BLESS_MASS_INJECTION_FIXTURES=1 to create it, then commit.",
            path.display()
        )
    });
    let expected = expected.trim();
    assert_eq!(
        hash, expected,
        "storage root drift for `{scenario}`. \
         If the change is intentional, refresh with \
         BLESS_MASS_INJECTION_FIXTURES=1 cargo test -p pallet-midds --test mass_injection."
    );
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

// -----------------------------------------------------------------------------
// Scenarios — see `docs/testing.md` §6.1
// -----------------------------------------------------------------------------

const SEED_10K: u64 = 0x000A_C0FF_EEDE_AD42;
const SEED_50K: u64 = 0x0032_FEED_FACE_BEEF;
const SEED_100K: u64 = 0x0064_CAFE_BABE_F00D;

#[test]
fn mass_injection_10k() {
    let items = gen_n(SEED_10K, 10_000);
    run_scenario("mass_injection_10k", items, &[1]);
}

#[test]
#[ignore = "nightly-only: 50k records across 100 depositors (see docs/testing.md §11)"]
fn mass_injection_50k() {
    let items = gen_n(SEED_50K, 50_000);
    let depositors: Vec<AccountId> = (1..=100).collect();
    run_scenario("mass_injection_50k", items, &depositors);
}

#[test]
#[ignore = "nightly-only: 100k records across 1000 depositors (see docs/testing.md §11)"]
fn mass_injection_100k() {
    let items = gen_n(SEED_100K, 100_000);
    let depositors: Vec<AccountId> = (1..=1000).collect();
    run_scenario("mass_injection_100k", items, &depositors);
}

#[test]
fn mass_injection_max_size() {
    let items: Vec<MusicalWork> = (0..1_000u32)
        .map(|i| {
            let mut work = max_size_musical_work();
            let MusicalWork::V1(v1) = &mut work;
            v1.iswc = iswc_for_index(i);
            work
        })
        .collect();
    run_scenario("mass_injection_max_size", items, &[1]);
}
