//! The settings a run of a title starts with (`victory run --work`).
//!
//! Each is one of the engine's own switches. What is set in the environment
//! already stays as it is, so every one of them can still be tried alone.

use clap::ValueEnum;

/// Where work that can be done on either side is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Work {
    /// Less on the graphics card, and less of its memory: for cards with
    /// little of it. More memory is copied and compared by the processor.
    Cpu,
    /// As measured on an RTX 5070.
    Balanced,
    /// More on the graphics card: buffer copies behind the work in flight,
    /// changed bytes by its copy engine, more of the title's memory in its
    /// own. Takes card memory that textures would have.
    Gpu,
    /// Experimental: balanced scheduling, exact SIMD comparisons, and
    /// memory allowances selected from the actual host adapter's budget.
    Auto,
}

/// What every choice has. The switches of soldsawn/victory-nvidia-8gb's
/// launcher, as measured (2026-11-25, RTX 5070, Series X profile).
/// `SKIP_WC_COMPARE` leaves shader writes to write-combining memory on the
/// card: 9 MB a frame come back instead of 68; what another copy of the
/// same memory needs is still fetched (`VICTORY_SKIP_WC_SHARED`).
const ALL: &[(&str, &str)] = &[
    ("VICTORY_ASYNC_SUBMIT", "1"),
    ("VICTORY_INDIRECT_WINDOW", "1"),
    ("VICTORY_INDIRECT_WINDOW_DRAWS", "1"),
    ("VICTORY_INDIRECT_GATHER", "1"),
    ("VICTORY_CLAMP_WAITS", "0"),
    ("VICTORY_JOIN_PARTS", "1"),
    ("VICTORY_BACK_OWN_VIEW", "1"),
    ("VICTORY_GUARD_HOLES_FRAME", "1"),
    ("VICTORY_HEAP_WORKER", "1"),
    ("VICTORY_ALIAS_ONCE", "1"),
    ("VICTORY_TEXTURE_NO_WAIT", "1"),
    ("VICTORY_COPY_PLAIN", "1"),
    ("VICTORY_SKIP_WC_COMPARE", "1"),
    ("VICTORY_WIDE_TEXTURES", "1"),
    ("VICTORY_DISK_CORES", "slow"),
    ("VICTORY_XMEM_PERSISTENT_VIEW", "1"),
    ("VICTORY_PIPELINE_WAIT_MS", "24"),
];

const CPU: &[(&str, &str)] = &[
    ("VICTORY_QUEUE2", "1"),
    ("VICTORY_CARD_BUFFERS_MB", "256"),
    ("VICTORY_TEXTURE_DROP_MIPS", "2"),
];

const BALANCED: &[(&str, &str)] = &[("VICTORY_QUEUE2", "1"), ("VICTORY_TEXTURE_DROP_MIPS", "1")];

const GPU: &[(&str, &str)] = &[
    ("VICTORY_QUEUE2", "3"),
    ("VICTORY_GPU_BUFFER_COPY", "1"),
    // A copy on the card leaves title memory as it was: only the bytes the
    // title itself changed may go up over it.
    ("VICTORY_REFRESH_COMPARE", "1"),
    ("VICTORY_REFRESH_STEP", "256"),
    ("VICTORY_WIDE_COMPARE", "1"),
    ("VICTORY_CARD_REHASH_RUNS", "1"),
    ("VICTORY_CARD_WINDOWS_MB", "512"),
    ("VICTORY_TEXTURE_DROP_MIPS", "1"),
];

// Same base as Balanced. Do not change any translator settings, waits,
// mip policy or queue mode. The budget-dependent decision is made in
// victory-gpu after the actual adapter is known, not by guessing a GPU.
const AUTO: &[(&str, &str)] = &[
    ("VICTORY_QUEUE2", "1"),
    ("VICTORY_TEXTURE_DROP_MIPS", "1"),
    ("VICTORY_CHANGED_RUNS", "1"),
    ("VICTORY_ADAPTIVE_TEXTURE_FLOOR", "1"),
    ("VICTORY_AUTO_CARD_MIN", "1"),
];

impl Work {
    /// The settings of the choice, each name once.
    pub fn settings(self) -> impl Iterator<Item = (&'static str, &'static str)> {
        let own = match self {
            Work::Cpu => CPU,
            Work::Balanced => BALANCED,
            Work::Gpu => GPU,
            Work::Auto => AUTO,
        };
        ALL.iter().chain(own).copied()
    }

    /// Puts them into the environment, where they are not set already.
    ///
    /// # Safety
    /// No other thread of the process reads or writes the environment:
    /// called first thing in `main`.
    pub unsafe fn apply(self) {
        for (name, value) in self.settings() {
            if std::env::var_os(name).is_none() {
                // SAFETY: the caller's.
                unsafe { std::env::set_var(name, value) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Work;

    #[test]
    fn a_choice_names_each_setting_once() {
        for work in [Work::Cpu, Work::Balanced, Work::Gpu, Work::Auto] {
            let mut names: Vec<_> = work.settings().map(|s| s.0).collect();
            let all = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), all, "{work:?}");
            assert!(names.iter().all(|n| n.starts_with("VICTORY_")));
        }
    }

    #[test]
    fn auto_is_balanced_plus_only_the_runtime_switches() {
        let balanced: Vec<_> = Work::Balanced.settings().collect();
        let auto: Vec<_> = Work::Auto.settings().collect();
        assert!(balanced.iter().all(|s| auto.contains(s)));
        let extra: Vec<_> = auto.into_iter().filter(|s| !balanced.contains(s)).collect();
        assert_eq!(extra, [
            ("VICTORY_CHANGED_RUNS", "1"),
            ("VICTORY_ADAPTIVE_TEXTURE_FLOOR", "1"),
            ("VICTORY_AUTO_CARD_MIN", "1"),
        ]);
    }

    #[test]
    fn copies_on_the_card_come_with_exact_uploads() {
        let of = |work: Work, name: &str| work.settings().find(|s| s.0 == name).map(|s| s.1);
        assert_eq!(of(Work::Gpu, "VICTORY_GPU_BUFFER_COPY"), Some("1"));
        assert_eq!(of(Work::Gpu, "VICTORY_REFRESH_COMPARE"), Some("1"));
        assert_eq!(of(Work::Balanced, "VICTORY_GPU_BUFFER_COPY"), None);
    }
}
