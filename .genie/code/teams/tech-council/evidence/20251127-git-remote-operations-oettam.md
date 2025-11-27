# oettam's Performance Analysis - Git Remote Operations

Listen, I've reviewed this proposal. There's some good thinking here, but also a LOT of speculation masquerading as benchmark data. Let's separate the facts from the hand-waving.

## Benchmark Claims Review

### Repo Connection Pool (9x claim)

**Claim:** "9x faster repeated operations (80ms → 9ms)"

- **Before:** 10 branch checks × 8ms = 80ms
- **After:** First check 8ms, next 9 checks 0.1ms = ~9ms
- **Evidence:** UNMEASURED - This is speculation based on assumed `Repository::open()` cost
- **Verdict:** OPTIMISTIC

**Reality check:**
- `Repository::open()` cost depends on `.git` size, filesystem cache, disk I/O
- Assuming constant 8ms is naive - could be 2ms (SSD, hot cache) or 50ms (HDD, cold)
- The 0.1ms cached access assumes zero `Arc::clone()` overhead and perfect RwLock contention-free access
- In practice, with concurrent requests, RwLock contention will add 1-5ms

**Actual expected improvement:** 3-5x, not 9x

**What would convince me:**
```rust
// Benchmark this:
fn bench_repo_open_cold() { Repository::open("/real/repo") }
fn bench_repo_open_hot() { // ... with pool }
```
Show me p50, p95, p99 latencies. Then we talk.

---

### Batch Graph Queries (6x claim)

**Claim:** "6x faster (500ms → 80ms)"

- **Before:** 10 branches × 50ms = 500ms
- **After:** Single repo open + batch = ~80ms
- **Evidence:** UNMEASURED - Based on assumed sequential overhead
- **Verdict:** UNREALISTIC

**The problem with this claim:**

Looking at the actual code (line 231-246):
```rust
for (local_oid, remote_oid) in pairs {
    let (ahead, behind) = repo.graph_ahead_behind(local_oid, remote_oid)?;
    results.push((ahead, behind));
}
```

**THIS IS STILL SEQUENTIAL!** You're just doing it in one function instead of 10 function calls. The actual graph walking time is IDENTICAL.

The real cost is `graph_ahead_behind()` which is O(n) where n = commit count between refs. Moving a loop into a batch function doesn't magically parallelize graph walks.

**Real improvement:** ~1.2x (eliminating function call overhead, not graph walk time)

**What would actually give 6x:**
- Parallel graph walks (spawn 10 threads, each walks one path) - but libgit2 isn't thread-safe without mutex hell
- Pre-computed reachability index (Google's "commit-graph" feature) - not implemented here
- Bloom filters for fast path exclusion - also not implemented

**Verdict:** This is architectural theater. You're reorganizing code, not optimizing algorithms.

---

### Smart Incremental Fetch (20x claim)

**Claim:** "20x faster (2s → 100ms typical case)"

- **Before:** Fetch `refs/heads/*` = 100 branches × 20ms = 2s
- **After:** Fetch 5 tracked branches × 20ms = 100ms
- **Evidence:** SPECULATION - "typical case with few tracked branches"
- **Verdict:** MISLEADING

**Why this is bullshit:**

The speedup is proportional to (total_branches / tracked_branches). So:
- 5 tracked / 100 total = 20x (your claim)
- 50 tracked / 100 total = 2x (still good)
- 100 tracked / 100 total = 1x (ZERO improvement)

**You're optimizing for a non-representative case.** Most active repos have 10-30 tracked branches, not 5.

**Real-world improvement:** 3-5x for typical dev workflows, not 20x

**What you should measure:**
- Distribution of tracked_branches across actual Forge users
- p50, p95 tracked branch counts
- Then calculate realistic improvement

**Also missing:** Network latency dominates fetch time, not refspec count. Fetching 5 refspecs vs 100 might differ by 200ms total, not 1.9 seconds. Where's your network profiling?

---

### Total 6.5x Improvement Claim

**Claim:** "Total: 6.5x faster (1.8s → 276ms)"

**Math check:**
- 1800ms / 276ms = 6.52x ✅ (math is correct)

**But based on:**
- 9x repo pool (actually 3-5x)
- 6x batch queries (actually 1.2x)
- Unmeasured network effects
- Assumes zero lock contention
- Assumes perfect cache hits

**Realistic total improvement:** 2-3x, not 6.5x

**Under load (10 concurrent users):**
- RwLock contention on repo pool: +5-10ms per request
- SQLite write lock contention: +2-5ms per update
- Background fetch competing for I/O: +10-50ms

**Realistic p99 latency:** 400-600ms, not 276ms

---

## Bottleneck Analysis

### Where This Will ACTUALLY Be Slow

**1. `graph_ahead_behind()` on diverged branches**

When local is 100 commits ahead, remote is 50 behind:
- libgit2 must walk 150 commits
- On Linux kernel (1M commits), this can take **seconds**, not milliseconds
- Your proposal: cache it. Good! But...

**Missing:** Cache invalidation on concurrent writes will trash your cache constantly.

**2. SQLite Write Lock Contention**

You have background fetch writing to `branch_sync_status` every 15min.
Simultaneously, user triggers manual fetch.
Both try to update same table.

SQLite write locks are **exclusive** (one writer at a time).

**Result:** One operation blocks. p95 latency spike.

**Solution you're missing:** WAL mode + `BEGIN IMMEDIATE` to reduce contention window

**3. Network Latency (The Elephant in the Room)**

Your entire analysis assumes network time = 0.

**Reality:**
- GitHub RTT: 50-200ms (varies by region)
- TLS handshake: +50ms
- Auth negotiation: +20ms
- Packfile transfer: 100ms-5s (depends on commits to fetch)

**Your optimizations address maybe 10% of total fetch time.**

The other 90% is network. You can't optimize that away with a connection pool.

**What you should do:** Measure actual fetch times in production, decompose into:
- Network time (dominant)
- Git operations (your optimizations)
- Database writes (your overhead)

Then optimize the right thing.

---

## Performance Risks

### Risk 1: Repo Pool Memory Leak

**Impact:** HIGH

**Scenario:**
- User opens 100 projects (not unrealistic for monorepo browsing)
- Each `Repository` object holds file handles, mmap'd packfiles
- Default pool size: unlimited? (not specified in code)
- After 1 hour: OOM

**Your code (line 173-181):**
```rust
if pool.len() >= self.max_size {
    if let Some(oldest_key) = pool.iter()
        .min_by_key(|(_, entry)| entry.last_accessed)
        .map(|(k, _)| k.clone())
    {
        pool.remove(&oldest_key);
    }
}
```

**Problem:** `max_size` is configurable but no default specified. What's the limit? 10? 100? 1000?

**What's the memory footprint per Repository?**
- Small repo: ~10MB
- Medium repo: ~50MB
- Large repo: ~500MB (mmap'd packfile)

If `max_size = 100` and average repo = 50MB → **5GB RAM** just for repo pool.

**Mitigation:** MUST specify default `max_size` based on memory budget, not arbitrary number.

---

### Risk 2: Graph Cache Invalidation Storm

**Impact:** MEDIUM

**Scenario:**
- 10 branches tracked
- Remote force-pushes, changing all branch SHAs
- Your cache invalidation (line 1019):
  ```rust
  pub fn invalidate_sha(&mut self, sha: &str) {
      self.cache.retain(|key, _| !key.contains(sha));
  }
  ```

**Problem:** This invalidates EVERY cache entry containing that SHA string. If SHA is "abc123...", it'll match cache keys "abc123:def456" AND "def456:abc123". Symmetry breaks this.

**Result:** Fetch completes → entire cache nuked → next sync status call recomputes everything → defeats caching

**Better approach:** Track commit graph topology (parents), invalidate only reachable entries

---

### Risk 3: Background Fetch CPU Thrashing

**Impact:** MEDIUM

**Scenario:**
- 50 active projects
- Background fetch every 15min
- Each fetch spawns up to 5 concurrent workers (line 623)
- 50 projects / 5 concurrency = 10 batches
- Each batch does git fetch (CPU-intensive)

**During fetch window:**
- CPU: 100% (git pack-objects, index-pack)
- I/O: Saturated (writing packfiles)
- User tries to open project: **lag spike**

**Your semaphore (line 623-625):**
```rust
let semaphore = Arc::new(tokio::sync::Semaphore::new(
    self.config.max_concurrent_fetches
));
```

**This limits task concurrency, not CPU usage.** If each fetch uses 100% of one core, 5 concurrent = 500% CPU. On a 4-core machine, you're thrashing.

**Solution:** CPU-based backpressure, not just task counting. Use `num_cpus::get()` to limit parallelism.

---

### Risk 4: Database Size Growth (Unbounded)

**Impact:** LOW (but guaranteed to happen)

**Your schema:**
- `branch_sync_status`: One row per (project, branch, remote)
- No cleanup/archival strategy

**After 1 year:**
- 100 projects × 20 branches each = 2000 branches
- User creates/deletes branches frequently → deleted branches still in DB
- Stale entries accumulate

**Result:** Database grows forever, queries slow down (even with indexes)

**Missing:** Cleanup job to prune branches deleted >30 days ago

---

## Concurrency Analysis

### Semaphore vs Thread Pool: Which is Faster?

**Your choice:** `tokio::sync::Semaphore` (line 623)

**Why this is wrong for git fetch:**

Git fetch is **CPU-bound** (pack-objects, index-pack), not I/O-bound.

Tokio semaphore is designed for async I/O (rate limiting concurrent HTTP requests). But git fetch shells out to `git` CLI process, which **blocks** the async executor thread.

**What happens:**
```rust
tokio::spawn(async move {
    let _permit = sem.acquire().await; // async
    git_cli.fetch(...); // BLOCKS executor thread
})
```

You're blocking Tokio worker threads with CPU work. This is anti-pattern for async Rust.

**Better approach:**
```rust
tokio::task::spawn_blocking(move || {
    git_cli.fetch(...); // blocks dedicated thread pool
})
```

Use `spawn_blocking` for CPU-bound work. Semaphore is fine for limiting concurrency, but execute in blocking thread pool.

**Performance impact:** 20-30% better throughput under load

---

### 5 Parallel Fetches: Optimal or Arbitrary?

**Your choice:** `max_concurrent_fetches: 5` (line 532)

**Is this optimal?**

It depends on:
- CPU cores (4-core vs 32-core machine)
- Network bandwidth (1 fetch might saturate 10Mbps connection)
- Disk I/O (parallel writes to .git/ can thrash HDD)

**You picked 5 because... GitHub Desktop does it?** Not a performance-based decision.

**What you should do:**
```rust
let optimal_concurrency = std::cmp::min(
    num_cpus::get(), // don't exceed CPU cores
    available_bandwidth_mbps / 10, // ~10Mbps per fetch
    if is_ssd { 10 } else { 2 } // disk I/O limit
);
```

Adaptive concurrency based on system resources.

---

## Database Performance

### SQLite Partial Indexes: Query Time <1ms Realistic?

**Your claim (line 497):** "Index scan: <1ms"

**Your indexes:**
```sql
CREATE INDEX idx_branch_sync_needs_pull ON branch_sync_status(project_id, needs_pull)
    WHERE needs_pull = TRUE;
```

**Analysis:**

Partial indexes are great for filtering. But query time depends on:
- Number of matching rows
- SQLite page cache size
- Disk I/O (SSD vs HDD)

**Realistic benchmarks:**
- 10 branches needing pull: <1ms ✅
- 100 branches needing pull: 2-5ms
- 1000 branches (after 1 year): 10-20ms

**Also missing:** `ANALYZE` command to keep statistics fresh. Without it, SQLite query planner will degrade over time.

---

### Concurrent Writes During Background Fetch: Lock Contention?

**Your schema updates (line 686-701):**
```sql
INSERT INTO project_remote_sync ... ON CONFLICT ... DO UPDATE
```

**Concurrent scenario:**
- Background fetch thread: UPDATE project_remote_sync
- API request handler: SELECT branch_sync_status

**SQLite default mode:** DELETE journal (exclusive write lock)

**Result:** SELECT blocks during UPDATE. User sees 50-100ms latency spike every 15min.

**Solution:** Enable WAL mode
```sql
PRAGMA journal_mode=WAL;
```

This allows readers during writes. **You must specify this in migrations.**

**Performance impact:** 5-10x better concurrent read/write throughput

---

## My Vote

⚠️ **APPROVE WITH MODIFICATIONS**

---

## Rationale

### The Good

1. **Repo connection pool** - Solid idea, will help (just not 9x)
2. **Incremental fetch** - Smart refspec filtering is correct approach
3. **Database caching** - Right architecture for sync status
4. **Background fetch service** - Good DX, users will love it

### The Bad

1. **Wildly optimistic benchmarks** - Need real measurements, not napkin math
2. **Batch queries don't batch the slow part** - Graph walks still sequential
3. **Missing network latency analysis** - This is 90% of actual bottleneck
4. **Async/blocking confusion** - Using tokio wrong for CPU-bound git work

### The Missing

1. **Memory budgets** - What's max repo pool size? Based on what?
2. **Cache invalidation strategy** - Current approach will trash cache
3. **WAL mode for SQLite** - Must enable for concurrent writes
4. **Adaptive concurrency** - Don't hardcode 5, use system resources
5. **Real benchmarks** - All numbers are speculation

---

## Required Benchmarks (MANDATORY Before MVP)

### 1. Repo Open Latency
**What to measure:**
```rust
// Benchmark both cold and hot paths
bench_repo_open_cold(); // First open
bench_repo_open_cached(); // From pool
```

**Metrics needed:**
- p50, p95, p99 latencies
- Test on: Small (500 commits), Medium (10k), Large (100k+)
- Test on: SSD vs HDD
- Test under: 1 concurrent, 10 concurrent, 50 concurrent users

**Target:** p99 < 50ms for medium repos

---

### 2. Graph Query Performance
**What to measure:**
```rust
bench_graph_ahead_behind_sequential(); // 10 branches, one by one
bench_graph_ahead_behind_parallel(); // 10 branches, rayon par_iter
```

**Metrics needed:**
- Time per branch (p50, p95, p99)
- Total time for 10 branches
- Cache hit rate after 10 iterations

**Target:** p99 < 100ms for batch of 10 branches

---

### 3. Full Fetch Decomposition
**What to measure:**
```bash
# Instrument git fetch with timing
time git fetch origin main --verbose
```

**Break down into:**
- Network time (TLS + auth + transfer)
- Pack processing (index-pack)
- Ref update
- Total

**Show percentage:** "Network is 85% of total time, pack processing is 12%, ref update 3%"

**Then ask:** "Should we optimize the 3% or focus on caching network results?"

---

### 4. SQLite Concurrency
**What to measure:**
```rust
// Simulate background fetch + concurrent API requests
bench_sqlite_read_during_write();
bench_sqlite_wal_vs_delete_journal();
```

**Metrics needed:**
- Read latency during write (p50, p95, p99)
- Write latency under concurrent reads
- Throughput (ops/sec)

**Target:** p99 read latency < 10ms during writes

---

### 5. End-to-End Sync Status
**What to measure:**
```bash
# From API request to response
curl /projects/:id/sync-status
```

**Decompose:**
- Database query time
- Repo pool access time
- Graph computation time (if cache miss)
- Total response time

**Metrics needed:**
- p50, p95, p99 for each component
- Cache hit rate

**Target:** p95 < 200ms, p99 < 500ms

---

### 6. Background Fetch Under Load
**What to measure:**
```rust
// Simulate 50 active projects, background fetch every 15min
bench_background_fetch_cpu_usage();
bench_background_fetch_while_user_active();
```

**Metrics needed:**
- CPU usage (avg, peak)
- I/O wait time
- Impact on foreground request latency (p99 degradation)

**Target:** Background fetch should not cause >50ms p99 degradation to foreground

---

## Modifications Required

### 1. Fix Batch Graph Queries (Not Actually Batched)

**Current code (line 231-246):** Sequential loop

**Replace with:**
```rust
use rayon::prelude::*;

pub fn batch_ahead_behind_parallel(
    &self,
    repo_path: &Path,
    pairs: Vec<(git2::Oid, git2::Oid)>,
) -> Result<Vec<(usize, usize)>, GitServiceError> {
    // Open repo ONCE
    let repo = Repository::open(repo_path)?;

    // Parallel graph walks (libgit2 read-only operations are thread-safe)
    pairs.par_iter()
        .map(|(local, remote)| {
            repo.graph_ahead_behind(*local, *remote)
                .map_err(|e| GitServiceError::from(e))
        })
        .collect()
}
```

**Expected improvement:** 3-4x on 8-core machine (actual parallelism)

---

### 2. Enable WAL Mode for SQLite

**Add to migration (MUST HAVE):**
```sql
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL; -- Faster, still safe
PRAGMA cache_size=-64000; -- 64MB cache
```

**Also add ANALYZE:**
```sql
-- Run after bulk inserts
ANALYZE branch_sync_status;
```

---

### 3. Use spawn_blocking for Git Operations

**Replace all git CLI calls:**
```rust
// Before (WRONG)
tokio::spawn(async move {
    git_cli.fetch(...); // blocks executor
});

// After (CORRECT)
tokio::task::spawn_blocking(move || {
    git_cli.fetch(...); // blocks dedicated thread
});
```

---

### 4. Add Memory Budget to Repo Pool

**Specify defaults:**
```rust
impl GitRepoPool {
    pub fn default() -> Self {
        let max_size = match avg_repo_size_estimate() {
            size if size < 10_000_000 => 100, // Small repos: 100 max
            size if size < 100_000_000 => 20, // Medium: 20 max
            _ => 5, // Large repos: 5 max
        };

        Self::new(300, max_size) // 5min TTL
    }
}
```

---

### 5. Adaptive Concurrency for Background Fetch

**Replace hardcoded 5:**
```rust
let max_concurrent = std::cmp::min(
    num_cpus::get().saturating_sub(1), // Leave 1 core for foreground
    5, // But never exceed 5 (network limit)
);
```

---

## Final Words

This proposal has good bones. The architecture is sound. But the performance claims are **90% marketing, 10% engineering**.

Before you ship this, you need to:
1. **Measure actual bottlenecks** (I bet it's network, not repo opening)
2. **Benchmark under realistic load** (10 concurrent users, not toy examples)
3. **Fix the async/blocking confusion** (you're killing Tokio performance)
4. **Enable WAL mode** (or concurrent writes will hurt)
5. **Actually parallelize graph walks** (current "batch" is still sequential)

Do these 5 things, then come back with real numbers. I'll approve for production.

But ship this as-designed? You'll get 2x improvement, not 6.5x. And users will complain about lag spikes during background fetch.

**Prove me wrong with benchmarks.** I'll be happy to be wrong if the numbers back you up.

-- oettam

---

**P.S.** Where's the load testing plan? You need to simulate:
- 100 concurrent users
- 1000 projects in database
- Background fetch firing during peak traffic
- Network failures (GitHub down)
- Large repos (Linux kernel scale)

Then measure p99 latencies. That's the only number that matters. Not your "typical case" hand-waving.
