//! Host pipelines are made by workers, not by the title thread that first
//! records a draw with them. Translating a shader and the host driver's
//! compile take from milliseconds to seconds each (structurisation of the
//! biggest compute programs ~0.4 s, some driver compiles 3.4 s), and
//! the title's first world frame wants over 1,200 new pipelines: made
//! on its recording threads, that frame took more than 30 s and the
//! title's GPU watchdog ended the run (decision 0007, "Pipelines made off
//! the title's threads").
//!
//! A first use queues the pipeline and waits for it within a budget per
//! frame, so the cheap ones (menus, one-off passes such as a colour-grading
//! bake) are there at once as before; past the budget a draw or dispatch
//! whose pipeline is not ready is dropped, as an untranslated one is.
//!
//! The default budget is half a second per title frame. A pass the title
//! runs only once and whose pipeline takes longer is lost.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::Shader;
use super::host::{GraphicsState, HostPipeline};

/// What a host pipeline is made from, and the host pipeline once made.
pub(crate) struct Pending {
    shaders: Vec<Arc<Shader>>,
    pub(crate) graphics: Option<Box<GraphicsState>>,
    queue_state: AtomicU8,
    /// Draws and dispatches that asked for it and found it not made: the
    /// workers make the most wanted one first.
    asked: AtomicU64,
    made: OnceLock<Option<HostPipeline>>,
    /// What identifies it across runs (shader containers and state), and
    /// whether a draw or dispatch has requested it yet.
    pub(super) key: String,
    used: AtomicBool,
    /// When a worker began it ([`slow`]).
    begun: OnceLock<Instant>,
    /// Why it is not made, if it is not ([`left_out`]).
    left_out: Option<&'static str>,
}

/// Why a pipeline with these shaders is not made, if it is not: what
/// `VICTORY_RT_RESOLVE=0` and `VICTORY_ACTIVATE_RAYS=0` leave out (both made
/// by default).
fn left_out(shaders: &[Arc<Shader>]) -> Option<&'static str> {
    if without_structures(shaders) {
        Some("shades ray-tracing hits, left out (VICTORY_RT_RESOLVE=0)")
    } else if too_dear(shaders) {
        Some("too dear to compile, left out (VICTORY_ACTIVATE_RAYS=0)")
    } else {
        None
    }
}

/// Whether some compute shader of `shaders` holds `name`.
fn named(shaders: &[Arc<Shader>], name: &[u8]) -> bool {
    shaders
        .iter()
        .any(|s| s.stage == "compute" && s.container.windows(name.len()).any(|w| w == name))
}

/// Whether a shader is left out for what it costs the host's driver:
/// `CS_ScreenProbes_ActivateRays` (39f0e9aeb156a61c, 66,501 instructions,
/// 9,948 branches). As translated before the clean-up passes
/// (`emit::shader_opt`), AMD's took a core and 0.8 GB more every 10 s for it
/// and did not finish (RX 7800 XT: 47 GB after 8 minutes); cleaned up, it
/// takes 85 s and 1.3 GB, once. `VICTORY_ACTIVATE_RAYS=0` leaves it out.
fn too_dear(shaders: &[Arc<Shader>]) -> bool {
    static OUT: OnceLock<bool> = OnceLock::new();
    let out = *OUT.get_or_init(|| std::env::var_os("VICTORY_ACTIVATE_RAYS").is_some_and(|v| v == "0"));
    out && named(shaders, b"CS_ScreenProbes_ActivateRays")
}

/// Whether a shader only shades what the console's ray-tracing structures
/// were hit by: about 1,000 compute programs, all bound at the world's first
/// frame, each 5 to 9 s of AMD's compiler. They stand in the queue before the
/// passes that make the picture, and with the structure builder off there is
/// no hit to shade. `VICTORY_RT_RESOLVE=0` leaves them out, unless the
/// builder is on (off here by default; the fork this is from,
/// myokex/victory-amd, leaves them out unless told otherwise).
fn without_structures(shaders: &[Arc<Shader>]) -> bool {
    static OUT: OnceLock<bool> = OnceLock::new();
    let is = |name, value| std::env::var_os(name).is_some_and(|v| v == value);
    let out = *OUT.get_or_init(|| is("VICTORY_RT_RESOLVE", "0") && !is("VICTORY_BVH_BUILDER", "1"));
    out && named(shaders, b"CS_RaytracingResolve")
}

// SAFETY: D3D12 objects are free-threaded; the graphics state (whose input
// elements point into its own names) is only read after creation.
unsafe impl Send for Pending {}
// SAFETY: as above.
unsafe impl Sync for Pending {}

impl Pending {
    /// Whether it is a compute pipeline.
    fn computes(&self) -> bool {
        matches!(self.shaders.as_slice(), [only] if only.stage == "compute")
    }

    pub(crate) fn new(shaders: Vec<Arc<Shader>>, graphics: Option<Box<GraphicsState>>) -> Pending {
        let key = key(&shaders, graphics.as_deref());
        let left_out = left_out(&shaders);
        Pending {
            shaders,
            graphics,
            left_out,
            queue_state: AtomicU8::new(NOT_QUEUED),
            asked: AtomicU64::new(0),
            made: OnceLock::new(),
            key,
            used: AtomicBool::new(false),
            begun: OnceLock::new(),
        }
    }

    /// Live title pipeline objects with the same key can share a pending
    /// translation and PSO. Weak entries never keep a GPU object alive.
    pub(crate) fn shared(shaders: Vec<Arc<Shader>>, graphics: Option<Box<GraphicsState>>) -> Arc<Pending> {
        static ON: OnceLock<bool> = OnceLock::new();
        if !*ON.get_or_init(|| std::env::var_os("VICTORY_PSO_SHARE").is_some_and(|v| v == "1")) {
            return Arc::new(Self::new(shaders, graphics));
        }
        Self::intern(Self::new(shaders, graphics))
    }

    fn intern(pending: Pending) -> Arc<Pending> {
        use std::collections::HashMap;
        use std::sync::{LazyLock, Weak};
        static LIVE: LazyLock<Mutex<HashMap<String, Weak<Pending>>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        const MAX: usize = 8192;
        let mut live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = live.get(&pending.key).and_then(Weak::upgrade)
            && !matches!(old.made.get(), Some(None))
        {
            return old;
        }
        if live.len() >= MAX {
            live.retain(|_, p| p.strong_count() != 0);
        }
        let pending = Arc::new(pending);
        if live.len() < MAX || live.contains_key(&pending.key) {
            live.insert(pending.key.clone(), Arc::downgrade(&pending));
        }
        pending
    }

    fn make(&self) {
        let _ = self.begun.set(Instant::now());
        self.make_with(|| HostPipeline::new(&self.shaders, self.graphics.as_deref()));
    }

    /// A panic on the way is a pipeline the host does not have: let out, it
    /// would end the worker for good and leave `made` unset, so that every
    /// later frame waited for it (and, on a title's thread, end the run).
    fn make_with(&self, new: impl FnOnce() -> Option<HostPipeline>) {
        self.made.get_or_init(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(new)).unwrap_or_else(|_| {
                tracing::error!(
                    key = %self.key,
                    shaders = ?self.shaders,
                    "making a host pipeline panicked; its draws are dropped"
                );
                None
            })
        });
        let (lock, done) = &DONE;
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        done.notify_all();
    }
}

/// Identifies the title pipeline by its shader containers and graphics state.
pub(crate) fn key(shaders: &[Arc<Shader>], graphics: Option<&GraphicsState>) -> String {
    let mut key = blake3::Hasher::new();
    for shader in shaders {
        key.update(shader.stage.as_bytes());
        key.update(shader.hash.as_bytes());
    }
    key.update(graphics.map(GraphicsState::describe).unwrap_or_default().as_bytes());
    key.finalize().to_hex()[..32].to_owned()
}

/// Why a pipeline has no host pipeline (diagnostics): its shaders that do
/// not translate, else "refused" or "being made"; `None` if it has one.
pub(crate) fn missing(p: &Pending) -> Option<String> {
    match p.made.get() {
        Some(Some(_)) => None,
        None => Some(p.left_out.map_or_else(
            || format!("being made {:?}", p.shaders),
            str::to_owned,
        )),
        Some(None) => {
            let failed: Vec<String> = p
                .shaders
                .iter()
                .filter(|s| matches!(s.translated.get(), Some(None)))
                .map(|s| format!("{} {}", s.stage, s.hash))
                .collect();
            Some(if failed.is_empty() {
                format!("refused {:?}", p.shaders)
            } else {
                failed.join(" + ")
            })
        }
    }
}

/// Signalled whenever a worker has made a pipeline.
static DONE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());

/// Waiting allowed per title frame, over all threads.
fn wait_budget() -> Duration {
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_PIPELINE_WAIT_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(500);
        tracing::info!(millis, "host pipeline wait budget per frame");
        Duration::from_millis(millis)
    })
}

/// Whether the host's driver has been at `p` for longer than most take
/// (one of the title's takes 85 s): no frame waits for it.
fn slow(p: &Pending) -> bool {
    // A pass a title runs once (it unpacks a model, say) is lost if its
    // pipeline is not there; a draw comes again in the next frame.
    let most = if p.computes() { 10 } else { 3 };
    p.begun.get().is_some_and(|at| at.elapsed() > Duration::from_secs(most))
}

/// What a frame may wait while a world loads: pipelines are asked for by
/// the hundred then, and a pass the title runs once is lost if its
/// pipeline is not there. All such waits of a run together are bounded;
/// the title's GPU watchdog bounds one frame's (30 s).
const LOADING_FRAME: Duration = Duration::from_secs(4);
/// The same for compute pipelines, waited for apart: the passes that unpack
/// a world's models are dispatched once, and in a run whose pipelines were
/// all still to be loaded 130 of them were dropped, and with them walls,
/// floor, ceiling and faces (they never came back).
const LOADING_FRAME_COMPUTE: Duration = Duration::from_secs(12);

/// `VICTORY_PIPELINE_GATE_S`: the bound on all of them (0: none of them).
fn loading_left(spent: Duration) -> Duration {
    static TOTAL: OnceLock<Duration> = OnceLock::new();
    let total = *TOTAL.get_or_init(|| {
        let seconds = std::env::var("VICTORY_PIPELINE_GATE_S")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(120);
        Duration::from_secs(seconds)
    });
    total.saturating_sub(spent)
}

/// Pipelines queued and not made yet.
static WAITING: AtomicU64 = AtomicU64::new(0);

/// Pipelines queued and not made yet (for the frame line).
pub(crate) fn waiting() -> u64 {
    WAITING.load(Ordering::Relaxed)
}

/// The host pipeline if it is made (or is made within the frame's budget);
/// queues it on first use.
pub(crate) fn ready(p: &Arc<Pending>) -> Option<&HostPipeline> {
    if p.left_out.is_some() {
        return None;
    }
    // A still-pending pipeline was requested by the title too. Prewarm it
    // on the next launch, even if this run never got to use it.
    if !p.used.load(Ordering::Relaxed) && !p.used.swap(true, Ordering::Relaxed) {
        super::library::note_used(&p.key);
    }
    if p.made.get().is_none() {
        p.asked.fetch_add(1, Ordering::Relaxed);
        start(p, REQUESTED);
        wait(p, crate::frame::current());
    }
    p.made.get()?.as_ref()
}

/// Starts `p` on the workers, before anything asks for it.
pub(crate) fn begin(p: &Arc<Pending>) {
    if p.left_out.is_none() {
        start(p, REQUESTED);
    }
}

/// Why each of `pipelines` that is made and is none the host has is not
/// there, for the log.
pub(crate) fn failures(pipelines: &[Arc<Pending>]) -> Vec<String> {
    pipelines
        .iter()
        .filter(|p| matches!(p.made.get(), Some(None)))
        .map(|p| {
            let untranslated: Vec<String> = p
                .shaders
                .iter()
                .filter(|s| matches!(s.translated.get(), Some(None)))
                .map(|s| format!("{} {} does not translate", s.stage, s.hash))
                .collect();
            if untranslated.is_empty() {
                format!("the host refuses {:?}", p.shaders)
            } else {
                untranslated.join(", ")
            }
        })
        .collect()
}

/// Makes `p` on this thread if it is not made yet (waiting for a worker
/// that is making it); whether it has a host pipeline.
pub(crate) fn now(p: &Arc<Pending>) -> bool {
    if p.left_out.is_some() {
        return false;
    }
    p.make();
    p.made.get().is_some_and(|made| made.is_some())
}

/// At the title's creation of a pipeline: starts it at once if an earlier
/// run drew with it (during loading, so the world's first frames find it).
pub(crate) fn early(p: &Arc<Pending>) {
    if p.left_out.is_none() && !p.shaders.is_empty() && super::library::wanted_early(&p.key) {
        super::library::EARLY.fetch_add(1, Ordering::Relaxed);
        start(p, EARLY);
    }
}

/// Whether the frame's budget is one for all threads together: the time in
/// which at least one thread waits counts against it, once ([`Spent`]).
/// Counted only when a wait had ended, the budget was one per thread:
/// each of the title's five recording threads began its first wait of a
/// frame with nothing counted yet and waited the whole half second, 2.5 s
/// of waiting in every frame in which pipelines were being made (runs
/// 34-49: "2504 ms waiting for pipelines" in each such frame).
/// On by default; `VICTORY_PIPELINE_WAIT_SHARED=0` counts as before.
fn shared_budget() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_WAIT_SHARED").is_none_or(|v| v != "0"))
}

/// What a frame's budget was spent on: the time in which any thread waited.
/// (The first thread to wait took all that was left of the budget and gave
/// back the rest afterwards: the others waited for nothing meanwhile, not
/// even for a pipeline that loads in a millisecond, and their draws were
/// dropped.)
struct Spent {
    frame: u64,
    time: Duration,
    /// Threads waiting now, and since when there were any (or since the
    /// frame began, if later).
    waiting: u32,
    since: Option<Instant>,
}

impl Spent {
    const fn new() -> Self {
        Self {
            frame: u64::MAX,
            time: Duration::ZERO,
            waiting: 0,
            since: None,
        }
    }

    /// A thread wants to wait in `frame` at `now`: how long it may, of
    /// `budget`. [`Spent::end`] follows if that is anything.
    fn begin(&mut self, frame: u64, budget: Duration, now: Instant) -> Duration {
        if self.frame != frame {
            (self.frame, self.time) = (frame, Duration::ZERO);
            self.since = (self.waiting > 0).then_some(now);
        }
        let running = self.since.map_or(Duration::ZERO, |s| now.saturating_duration_since(s));
        let left = budget.saturating_sub(self.time + running);
        if !left.is_zero() {
            self.waiting += 1;
            self.since.get_or_insert(now);
        }
        left
    }

    fn end(&mut self, now: Instant) {
        self.waiting -= 1;
        if self.waiting == 0
            && let Some(since) = self.since.take()
        {
            self.time += now.saturating_duration_since(since);
        }
    }
}

/// Waits for `p` while frame `frame` has budget left. Dispatches have a
/// budget of their own, as large: what a dropped draw leaves out is drawn
/// in the next frame, but what a pass that runs once would have written is
/// lost, and draws (far more of them) came first and spent it all.
fn wait(p: &Pending, frame: u64) {
    let budget = wait_budget();
    if budget.is_zero() || slow(p) {
        return;
    }
    // What was waited past the budget since the start, in milliseconds; the
    // second of each is the compute pipelines'.
    static LOADINGS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
    let compute = p.computes();
    let past_total = &LOADINGS[usize::from(compute)];
    let loading = loading_left(Duration::from_millis(past_total.load(Ordering::Relaxed)));
    let budget = budget.max((if compute { LOADING_FRAME_COMPUTE } else { LOADING_FRAME }).min(loading));
    static SPENTS: [Mutex<Spent>; 2] = [const { Mutex::new(Spent::new()) }; 2];
    // Each thread's waits count on their own without the shared budget.
    static ALONES: [Mutex<(u64, Duration)>; 2] = [const { Mutex::new((u64::MAX, Duration::ZERO)) }; 2];
    let kind = usize::from(compute);
    let (spent_of, alone_of) = (&SPENTS[kind], &ALONES[kind]);
    let shared = shared_budget();
    let started = Instant::now();
    let left = if shared {
        spent_of.lock().unwrap_or_else(|e| e.into_inner()).begin(frame, budget, started)
    } else {
        let mut spent = alone_of.lock().unwrap_or_else(|e| e.into_inner());
        if spent.0 != frame {
            *spent = (frame, Duration::ZERO);
        }
        budget.saturating_sub(spent.1)
    };
    if left.is_zero() {
        return;
    }
    let (lock, done) = &DONE;
    let mut held = lock.lock().unwrap_or_else(|e| e.into_inner());
    while p.made.get().is_none() && !slow(p) {
        let rest = left.saturating_sub(started.elapsed());
        if rest.is_zero() {
            break;
        }
        // Woken now and then to see whether it has become a slow one.
        let rest = rest.min(Duration::from_millis(250));
        held = done.wait_timeout(held, rest).unwrap_or_else(|e| e.into_inner()).0;
    }
    drop(held);
    let waited = started.elapsed().min(left);
    let past = waited.saturating_sub(wait_budget());
    past_total.fetch_add(past.as_millis() as u64, Ordering::Relaxed);
    if shared {
        spent_of.lock().unwrap_or_else(|e| e.into_inner()).end(Instant::now());
    } else {
        let mut spent = alone_of.lock().unwrap_or_else(|e| e.into_inner());
        // A frame that began meanwhile has its own budget.
        if spent.0 == frame {
            spent.1 += started.elapsed().min(left);
        }
    }
    crate::framelog::add(crate::framelog::T::PipelineWaitUs, started.elapsed().as_micros() as u64);
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(
        crate::copystat::C::PipelineWaitUs,
        started.elapsed().as_micros() as u64,
    );
}

const NOT_QUEUED: u8 = 0;
const EARLY: u8 = 1;
const REQUESTED: u8 = 2;
const MAKING: u8 = 3;

struct Queue {
    early: VecDeque<Arc<Pending>>,
    requested: VecDeque<Arc<Pending>>,
}

impl Queue {
    const fn new() -> Self {
        Self {
            early: VecDeque::new(),
            requested: VecDeque::new(),
        }
    }

    /// Queues an unrequested pipeline or promotes one queued at creation.
    /// Returns whether it is new to the queue, or `None` if nothing changed.
    fn push(&mut self, p: &Arc<Pending>, priority: u8) -> Option<bool> {
        let prev = p.queue_state.load(Ordering::Relaxed);
        if prev >= priority {
            return None;
        }
        p.queue_state.store(priority, Ordering::Relaxed);
        if priority == REQUESTED {
            self.requested.push_back(p.clone());
        } else {
            self.early.push_back(p.clone());
        }
        Some(prev == NOT_QUEUED)
    }

    /// The most asked for of the requested ones (the first of equals), then
    /// the early ones in order. In the order of the first request, one that
    /// thousands of draws a frame want waited behind hundreds wanted once.
    fn pop(&mut self) -> Option<Arc<Pending>> {
        let mut most: Option<(usize, u64)> = None;
        for (at, p) in self.requested.iter().enumerate() {
            let asked = p.asked.load(Ordering::Relaxed);
            if most.is_none_or(|(_, m)| asked > m) {
                most = Some((at, asked));
            }
        }
        let p = most.and_then(|(at, _)| self.requested.remove(at)).or_else(|| {
            // An early entry can have been promoted and claimed already.
            std::iter::from_fn(|| self.early.pop_front())
                .find(|p| p.queue_state.load(Ordering::Relaxed) == EARLY)
        })?;
        p.queue_state.store(MAKING, Ordering::Relaxed);
        Some(p)
    }
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());
static AVAILABLE: Condvar = Condvar::new();

/// Start or promote work for a draw without waiting behind the entire early
/// list. Queue changes and worker claims happen under the same lock.
fn start(p: &Arc<Pending>, priority: u8) {
    if p.queue_state.load(Ordering::Relaxed) >= priority {
        return;
    }
    if !workers() {
        if p.queue_state.swap(MAKING, Ordering::Relaxed) == NOT_QUEUED {
            p.make();
        }
        return;
    }
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(new) = queue.push(p, priority) {
        if new {
            PEAK.fetch_max(WAITING.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Relaxed);
        }
        AVAILABLE.notify_one();
    }
}

fn next() -> Arc<Pending> {
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(p) = queue.pop() {
            return p;
        }
        queue = AVAILABLE.wait(queue).unwrap_or_else(|e| e.into_inner());
    }
}

/// Starts the workers (half the logical processors, 2 to 8) on first use.
fn workers() -> bool {
    static RUNNING: OnceLock<bool> = OnceLock::new();
    *RUNNING.get_or_init(|| {
        let mut running = false;
        let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 8));
        for i in 0..workers {
            let spawned = std::thread::Builder::new()
                .name(format!("victory-pipelines-{i}"))
                .spawn(|| {
                    // Below the title's own threads (loading, recording).
                    // SAFETY: a plain call on our own thread.
                    unsafe {
                        use windows::Win32::System::Threading::*;
                        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
                    }
                    loop {
                        next().make();
                        if WAITING.fetch_sub(1, Ordering::Relaxed) == 1 {
                            drained();
                        } else {
                            super::library::save(false);
                        }
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "no pipeline worker");
            } else {
                running = true;
            }
        }
        running
    })
}

/// The most pipelines waiting at once since the workers last ran out.
static PEAK: AtomicU64 = AtomicU64::new(0);

/// The workers ran out of work: logged after a batch of 10 or more, and
/// the cache saved (at most every 20 s).
fn drained() {
    use super::library::{EARLY, LOADED, MADE};
    if PEAK.swap(0, Ordering::Relaxed) >= 10 {
        tracing::info!(
            made = MADE.load(Ordering::Relaxed),
            loaded = LOADED.load(Ordering::Relaxed),
            started_early = EARLY.load(Ordering::Relaxed),
            "host pipelines all made"
        );
    }
    super::library::save(false);
}

#[cfg(test)]
mod tests {
    #[test]
    fn live_identical_pipelines_share_pending_work() {
        let a = Pending::intern(Pending::new(Vec::new(), None));
        let b = Pending::intern(Pending::new(Vec::new(), None));
        assert!(Arc::ptr_eq(&a, &b));
    }

    use super::*;

    /// A first use queues the pipeline and, within the budget, gets the
    /// worker's answer (here: no host pipeline for no shaders).
    #[test]
    fn first_use_is_made_by_a_worker() {
        let p = Arc::new(Pending::new(Vec::new(), None));
        assert!(ready(&p).is_none());
        assert_ne!(p.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        let started = Instant::now();
        while p.made.get().is_none() && started.elapsed() < Duration::from_secs(10) {
            std::thread::yield_now();
        }
        assert!(matches!(p.made.get(), Some(None)));
    }

    /// Threads waiting at the same time share the time they wait, none
    /// takes it from the others, and a new frame starts anew.
    #[test]
    fn waits_at_the_same_time_count_once() {
        let ms = Duration::from_millis;
        let (t, mut s) = (Instant::now(), Spent::new());
        assert_eq!(s.begin(1, ms(500), t), ms(500));
        assert_eq!(s.begin(1, ms(500), t + ms(100)), ms(400), "the second waits too");
        s.end(t + ms(150));
        s.end(t + ms(200));
        assert_eq!(s.begin(1, ms(500), t + ms(900)), ms(300), "200 ms were waited");
        s.end(t + ms(1200));
        assert_eq!(s.begin(1, ms(500), t + ms(1300)), ms(0));
        // A wait that reaches into the next frame counts there from its start.
        assert_eq!(s.begin(2, ms(500), t + ms(1400)), ms(500));
        assert_eq!(s.begin(3, ms(500), t + ms(1500)), ms(500));
        s.end(t + ms(1600));
        s.end(t + ms(1700));
        assert_eq!(s.begin(3, ms(500), t + ms(1800)), ms(300));
    }

    /// It is answered, waited for by no one, and the worker goes on.
    #[test]
    fn a_panic_while_making_is_a_pipeline_the_host_does_not_have() {
        let p = Arc::new(Pending::new(Vec::new(), None));
        p.make_with(|| panic!("a fault of the translator's"));
        assert!(matches!(p.made.get(), Some(None)));
        p.make_with(|| unreachable!("tried again"));
        let started = Instant::now();
        assert!(ready(&p).is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
        let next = Arc::new(Pending::new(Vec::new(), None));
        next.make();
        assert!(matches!(next.made.get(), Some(None)));
    }

    #[test]
    fn pending_pipeline_is_recorded_for_the_next_launch() {
        let mut p = Pending::new(Vec::new(), None);
        p.key = "pending-pipeline-test".to_owned();
        // Simulate a worker that has taken the job but cannot finish it yet.
        p.queue_state.store(MAKING, Ordering::Relaxed);
        let p = Arc::new(p);

        assert!(!super::super::library::wanted_early(&p.key));
        assert!(ready(&p).is_none());
        assert!(p.made.get().is_none());
        assert!(super::super::library::wanted_early(&p.key));
    }

    #[test]
    fn the_most_wanted_pipeline_is_made_first() {
        let new = |asked: u64| {
            let p = Arc::new(Pending::new(Vec::new(), None));
            p.asked.store(asked, Ordering::Relaxed);
            p
        };
        let (once, often, also_once, unasked) = (new(1), new(900), new(1), new(0));
        let mut queue = Queue::new();
        for p in [&unasked, &once, &also_once, &often] {
            assert_eq!(queue.push(p, REQUESTED), Some(true));
        }
        // More draws ask while it waits.
        also_once.asked.fetch_add(5, Ordering::Relaxed);
        for p in [&often, &also_once, &once, &unasked] {
            assert!(Arc::ptr_eq(&queue.pop().unwrap(), p));
        }
        assert!(queue.pop().is_none());
    }

    #[test]
    fn only_pipelines_the_host_does_not_have_are_failures() {
        let (refused, waiting) = (
            Arc::new(Pending::new(Vec::new(), None)),
            Arc::new(Pending::new(Vec::new(), None)),
        );
        refused.make_with(|| None);
        assert_eq!(failures(&[refused, waiting]).len(), 1);
    }

    #[test]
    fn draws_promote_early_pipelines_without_claiming_them_twice() {
        let earlier = Arc::new(Pending::new(Vec::new(), None));
        let promoted = Arc::new(Pending::new(Vec::new(), None));
        let requested = Arc::new(Pending::new(Vec::new(), None));
        let mut queue = Queue::new();

        assert_eq!(queue.push(&earlier, EARLY), Some(true));
        assert_eq!(queue.push(&promoted, EARLY), Some(true));
        assert_eq!(queue.push(&requested, REQUESTED), Some(true));
        assert_eq!(queue.push(&promoted, REQUESTED), Some(false));
        assert_eq!(queue.push(&promoted, REQUESTED), None);

        assert!(Arc::ptr_eq(&queue.pop().unwrap(), &requested));
        assert!(Arc::ptr_eq(&queue.pop().unwrap(), &promoted));
        assert_eq!(queue.push(&promoted, REQUESTED), None);
        assert!(Arc::ptr_eq(&queue.pop().unwrap(), &earlier));
        assert!(queue.pop().is_none());
    }
}
