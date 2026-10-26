//! The session's GPUs (#123): how each one's screens get their frames,
//! what each connector is for, what a GPU's screens are called, and what
//! becomes of them when its connectors change or the GPU goes. Plain
//! decisions, apart from DRM, so that they are tested without hardware;
//! `tty` carries them out.

/// How the frames of a GPU's screens are made and reach them. The primary
/// GPU draws every screen: what windows show, the effects and the shell
/// are drawn once, by one renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// The primary GPU's own screens: drawn where they are shown.
    Direct,
    /// A display engine that scans out the buffers the primary GPU draws
    /// into (a DisplayLink dock): nothing is copied.
    Shared,
    /// The primary GPU draws, and smithay's multi-GPU renderer copies each
    /// frame into the GPU's own buffers (the HDMI port of a laptop's second
    /// GPU, a second virtio-gpu).
    Copied,
}

/// What a GPU can draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Hardware,
    /// Mesa's software GL, on a GPU without 3D.
    Software,
    /// No GL at all.
    None,
}

/// What opening a GPU found out about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub primary: bool,
    pub renderer: Renderer,
    /// It took a buffer the primary GPU drew as a framebuffer.
    pub scans_out_shared: bool,
}

impl Probe {
    /// How its screens get their frames; none when they cannot.
    pub fn path(&self) -> Option<Path> {
        if self.primary {
            return Some(Path::Direct);
        }
        match (self.renderer, self.scans_out_shared) {
            // Its own memory scans out best, and a GPU may not scan out
            // another's at all.
            (Renderer::Hardware, _) => Some(Path::Copied),
            (_, true) => Some(Path::Shared),
            // Slowly, through the CPU, but shown.
            (Renderer::Software, false) => Some(Path::Copied),
            (Renderer::None, false) => None,
        }
    }
}

/// What a connected connector is for. A VR headset says it is no desktop
/// (its connector's `non-desktop` property): the session never lights it
/// and offers it to clients through drm-lease instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Desktop,
    Lease,
}

impl Use {
    pub fn of(non_desktop: bool) -> Self {
        if non_desktop {
            Self::Lease
        } else {
            Self::Desktop
        }
    }
}

/// A connector found connected, with modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found<C> {
    pub connector: C,
    pub use_: Use,
}

/// What the session does with a connector it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// A screen of the desktop.
    Lit,
    /// A screen of the desktop, turned off in the settings.
    Off,
    /// Offered for lease (taken by a client or not).
    Offered,
}

/// What follows from a scan of a GPU's connectors, in order: what goes
/// first, so that what comes finds its CRTC free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<C> {
    /// A lit screen goes from the desktop.
    Unplug(C),
    /// A screen turned off is forgotten.
    Forget(C),
    /// No longer offered for lease; a lease of it ends.
    Withdraw(C),
    /// A new screen of the desktop.
    Light(C),
    /// Offered for lease, never lit.
    Offer(C),
}

/// What changes when a GPU's connectors are now `found` and the session
/// knows `known`. A GPU that goes is a scan that finds nothing.
pub fn scan<C: Copy + Eq>(found: &[Found<C>], known: &[(C, Known)]) -> Vec<Change<C>> {
    let found_as = |c: C| found.iter().find(|f| f.connector == c).map(|f| f.use_);
    let mut changes = Vec::new();
    for &(connector, was) in known {
        let now = found_as(connector);
        match (was, now) {
            (Known::Lit, Some(Use::Desktop))
            | (Known::Off, Some(Use::Desktop))
            | (Known::Offered, Some(Use::Lease)) => {}
            (Known::Lit, _) => changes.push(Change::Unplug(connector)),
            (Known::Off, _) => changes.push(Change::Forget(connector)),
            (Known::Offered, _) => changes.push(Change::Withdraw(connector)),
        }
    }
    for f in found {
        let kept = known.iter().any(|&(c, was)| {
            c == f.connector
                && matches!(
                    (was, f.use_),
                    (Known::Lit | Known::Off, Use::Desktop) | (Known::Offered, Use::Lease)
                )
        });
        if !kept {
            changes.push(match f.use_ {
                Use::Desktop => Change::Light(f.connector),
                Use::Lease => Change::Offer(f.connector),
            });
        }
    }
    changes
}

/// The name a screen goes by: its connector's (`HDMI-A-1`), unless a GPU
/// opened before this one has a connector by that name (two GPUs each
/// number their own); then its card's name comes first (`card1-HDMI-A-1`).
/// The primary GPU is opened first: its screens keep their names, and
/// these are the same from one start to the next.
pub fn screen_name(connector: &str, card: &str, taken: bool) -> String {
    if taken {
        format!("{card}-{connector}")
    } else {
        connector.to_string()
    }
}

/// The first CRTC of `possible` nothing else uses.
pub fn pick_crtc<C: Copy>(
    possible: impl IntoIterator<Item = C>,
    used: impl Fn(C) -> bool,
) -> Option<C> {
    possible.into_iter().find(|c| !used(*c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(primary: bool, renderer: Renderer, scans_out_shared: bool) -> Option<Path> {
        Probe {
            primary,
            renderer,
            scans_out_shared,
        }
        .path()
    }

    #[test]
    fn the_primary_gpu_draws_every_screen_and_others_get_its_frames() {
        assert_eq!(probe(true, Renderer::Software, false), Some(Path::Direct));
        // A second GPU with a renderer of its own takes copies.
        assert_eq!(probe(false, Renderer::Hardware, true), Some(Path::Copied));
        // A dock's display engine scans out what the primary drew.
        assert_eq!(probe(false, Renderer::None, true), Some(Path::Shared));
        assert_eq!(probe(false, Renderer::Software, true), Some(Path::Shared));
        // A VM's second virtio-gpu: copied through the CPU.
        assert_eq!(probe(false, Renderer::Software, false), Some(Path::Copied));
        assert_eq!(probe(false, Renderer::None, false), None);
    }

    const DESK: Use = Use::Desktop;
    const LEASE: Use = Use::Lease;

    fn found(list: &[(u32, Use)]) -> Vec<Found<u32>> {
        list.iter()
            .map(|&(connector, use_)| Found { connector, use_ })
            .collect()
    }

    #[test]
    fn a_vr_headset_is_offered_and_never_lit() {
        let changes = scan(&found(&[(1, DESK), (2, LEASE)]), &[]);
        assert_eq!(changes, vec![Change::Light(1), Change::Offer(2)]);
        // Scanned again: nothing changes.
        let known = [(1, Known::Lit), (2, Known::Offered)];
        assert_eq!(scan(&found(&[(1, DESK), (2, LEASE)]), &known), vec![]);
    }

    #[test]
    fn a_screen_that_turns_out_to_be_a_headset_leaves_the_desktop() {
        // Its EDID changed: now it says it is no desktop.
        let changes = scan(&found(&[(1, LEASE)]), &[(1, Known::Lit)]);
        assert_eq!(changes, vec![Change::Unplug(1), Change::Offer(1)]);
        let changes = scan(&found(&[(1, LEASE)]), &[(1, Known::Off)]);
        assert_eq!(changes, vec![Change::Forget(1), Change::Offer(1)]);
        // And back.
        let changes = scan(&found(&[(1, DESK)]), &[(1, Known::Offered)]);
        assert_eq!(changes, vec![Change::Withdraw(1), Change::Light(1)]);
    }

    #[test]
    fn a_gpu_that_goes_takes_all_its_screens_and_leases() {
        let known = [
            (1, Known::Lit),
            (2, Known::Lit),
            (3, Known::Off),
            (4, Known::Offered),
        ];
        assert_eq!(
            scan(&[], &known),
            vec![
                Change::Unplug(1),
                Change::Unplug(2),
                Change::Forget(3),
                Change::Withdraw(4)
            ]
        );
    }

    #[test]
    fn a_screen_turned_off_stays_off_while_plugged() {
        assert_eq!(scan(&found(&[(3, DESK)]), &[(3, Known::Off)]), vec![]);
    }

    #[test]
    fn the_second_gpus_screens_are_told_apart_from_the_first() {
        assert_eq!(screen_name("HDMI-A-1", "card1", false), "HDMI-A-1");
        assert_eq!(screen_name("Virtual-1", "card1", true), "card1-Virtual-1");
    }

    #[test]
    fn a_crtc_in_use_is_not_picked() {
        assert_eq!(pick_crtc([10, 11, 12], |c| c == 10), Some(11));
        assert_eq!(pick_crtc([10], |_| true), None);
    }
}
