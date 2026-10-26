//! drm-lease (wp-drm-lease-v1, #123): a VR headset's connector, which says
//! it is no desktop, is never lit; it is offered to clients (a VR runtime),
//! which lease it with a CRTC and planes of its own and drive it
//! themselves. The global is hidden from sandboxed clients, as the other
//! protocols that reach past a client's own windows are.
//!
//! The leases live here, in the state, since smithay asks the state for
//! them; the GPUs live in the backend, which offers and withdraws
//! connectors as it scans them.

use std::collections::HashMap;

use smithay::{
    backend::drm::{DrmDevice, DrmNode, PlaneClaim},
    delegate_drm_lease,
    reexports::{
        drm::control::{connector, crtc, plane},
        wayland_server::DisplayHandle,
    },
    wayland::drm_lease::{
        DrmLease, DrmLeaseBuilder, DrmLeaseHandler, DrmLeaseRequest, DrmLeaseState, LeaseRejected,
    },
};

use crate::{State, handlers::protocols::trusted};

/// A connector offered for lease, with what driving it takes: a CRTC no
/// screen uses and its planes, claimed so that no screen takes them.
#[derive(Debug)]
pub struct Offer {
    pub connector: connector::Handle,
    pub crtc: crtc::Handle,
    pub planes: Vec<(plane::Handle, PlaneClaim)>,
}

/// One GPU's lease global and what it offers.
#[derive(Debug)]
struct Device {
    state: DrmLeaseState,
    offers: Vec<Offer>,
    /// Leases given; dropping one ends it.
    active: Vec<DrmLease>,
    /// Empty lease builders, made by the backend that has the GPU (a
    /// builder is made from it); a request takes one.
    seeds: Vec<DrmLeaseBuilder>,
    /// The GPU went: its global is disabled, and this stays so that what
    /// clients still hold of it is answered.
    gone: bool,
}

/// Every GPU's leases.
#[derive(Debug, Default)]
pub struct Leases {
    devices: HashMap<DrmNode, Device>,
}

/// Why a lease is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not offered: a screen of the desktop, or gone.
    NotOffered,
    /// Leased to another client already.
    Leased,
}

/// The offers a request for `wanted` takes, by index; `offered` lists each
/// offered connector and whether it is leased.
pub fn plan<C: Copy + Eq>(wanted: &[C], offered: &[(C, bool)]) -> Result<Vec<usize>, Refusal> {
    wanted
        .iter()
        .map(|w| {
            let i = offered
                .iter()
                .position(|(c, _)| c == w)
                .ok_or(Refusal::NotOffered)?;
            if offered[i].1 {
                return Err(Refusal::Leased);
            }
            Ok(i)
        })
        .collect()
}

impl Leases {
    /// Starts the lease global of a GPU; sandboxed clients do not see it.
    pub fn add_device(&mut self, display: &DisplayHandle, node: DrmNode) {
        match DrmLeaseState::new_with_filter::<State, _>(display, &node, trusted) {
            Ok(state) => {
                // A GPU back again: what was kept of it goes.
                self.devices.insert(
                    node,
                    Device {
                        state,
                        offers: Vec::new(),
                        active: Vec::new(),
                        seeds: Vec::new(),
                        gone: false,
                    },
                );
            }
            Err(err) => tracing::warn!(%err, %node, "no drm-lease for this GPU"),
        }
    }

    /// The GPU went: its leases end and its global is no longer offered.
    pub fn remove_device(&mut self, node: DrmNode) {
        let Some(device) = self.devices.get_mut(&node) else {
            return;
        };
        for offer in std::mem::take(&mut device.offers) {
            device.state.withdraw_connector(offer.connector);
        }
        device.active.clear();
        device.seeds.clear();
        device.state.disable_global::<State>();
        device.gone = true;
    }

    /// Offers `offer`'s connector; `name` is the connector's, `description`
    /// the screen's make and model.
    pub fn offer(&mut self, node: DrmNode, offer: Offer, name: String, description: String) {
        let Some(device) = self.devices.get_mut(&node).filter(|d| !d.gone) else {
            return;
        };
        if device.offers.iter().any(|o| o.connector == offer.connector) {
            return;
        }
        tracing::info!(connector = name, "offered for lease");
        device
            .state
            .add_connector::<State>(offer.connector, name, description);
        device.offers.push(offer);
    }

    /// No longer offers `connector`; its lease, if any, ends.
    pub fn withdraw(&mut self, node: DrmNode, connector: connector::Handle) {
        let Some(device) = self.devices.get_mut(&node) else {
            return;
        };
        device.offers.retain(|o| o.connector != connector);
        device.state.withdraw_connector(connector);
        device
            .active
            .retain(|lease| !lease.connectors().any(|c| *c == connector));
    }

    pub fn is_offered(&self, node: DrmNode, connector: connector::Handle) -> bool {
        self.devices
            .get(&node)
            .is_some_and(|d| d.offers.iter().any(|o| o.connector == connector))
    }

    /// The connectors `node` offers.
    pub fn offered(&self, node: DrmNode) -> Vec<connector::Handle> {
        self.devices
            .get(&node)
            .map(|d| d.offers.iter().map(|o| o.connector).collect())
            .unwrap_or_default()
    }

    /// A CRTC kept for a connector offered for lease.
    pub fn holds_crtc(&self, node: DrmNode, crtc: crtc::Handle) -> bool {
        self.devices
            .get(&node)
            .is_some_and(|d| d.offers.iter().any(|o| o.crtc == crtc))
    }

    /// Keeps a builder ready for each connector offered.
    pub fn refill(&mut self, node: DrmNode, drm: &DrmDevice) {
        if let Some(device) = self.devices.get_mut(&node).filter(|d| !d.gone) {
            while device.seeds.len() < device.offers.len() {
                device.seeds.push(DrmLeaseBuilder::new(drm));
            }
        }
    }

    /// Another terminal is shown: the GPUs are not ours to lend, and
    /// clients see the connectors withdrawn. Leases given stay: logind took
    /// the GPUs from the session already, so they could not be revoked,
    /// and a headset is no part of what the other terminal shows.
    pub fn suspend(&mut self) {
        for device in self.devices.values_mut().filter(|d| !d.gone) {
            device.state.suspend();
        }
    }

    /// Back: the connectors are offered again (one leased is refused).
    pub fn resume(&mut self) {
        for device in self.devices.values_mut().filter(|d| !d.gone) {
            device.state.resume::<State>();
        }
    }

    /// Leases given, for the log and the tests.
    pub fn active(&self) -> usize {
        self.devices.values().map(|d| d.active.len()).sum()
    }
}

impl DrmLeaseHandler for State {
    fn drm_lease_state(&mut self, node: DrmNode) -> &mut DrmLeaseState {
        // Lease objects carry the node of a global made here, and a GPU's
        // entry is kept after it goes (`Leases::remove_device`).
        #[allow(clippy::expect_used)]
        &mut self
            .leases
            .devices
            .get_mut(&node)
            .expect("a lease object names a GPU whose leases are kept")
            .state
    }

    fn lease_request(
        &mut self,
        node: DrmNode,
        request: DrmLeaseRequest,
    ) -> Result<DrmLeaseBuilder, LeaseRejected> {
        let device = self
            .leases
            .devices
            .get_mut(&node)
            .filter(|d| !d.gone)
            .ok_or_else(LeaseRejected::default)?;
        let offered: Vec<(connector::Handle, bool)> = device
            .offers
            .iter()
            .map(|o| {
                let leased = device
                    .active
                    .iter()
                    .any(|l| l.connectors().any(|c| *c == o.connector));
                (o.connector, leased)
            })
            .collect();
        let taken = plan(&request.connectors, &offered).map_err(|refusal| {
            tracing::info!(?refusal, "lease refused");
            LeaseRejected::default()
        })?;
        let mut builder = device.seeds.pop().ok_or_else(|| {
            tracing::info!("lease refused: asked again before the GPU was ready");
            LeaseRejected::default()
        })?;
        for i in taken {
            let offer = &device.offers[i];
            builder.add_connector(offer.connector);
            builder.add_crtc(offer.crtc);
            for (plane, claim) in &offer.planes {
                builder.add_plane(*plane, claim.clone());
            }
        }
        Ok(builder)
    }

    fn new_active_lease(&mut self, node: DrmNode, lease: DrmLease) {
        tracing::info!(%node, lease = lease.id(), "leased");
        if let Some(device) = self.leases.devices.get_mut(&node) {
            device.active.push(lease);
        }
    }

    fn lease_destroyed(&mut self, node: DrmNode, lease: u32) {
        tracing::info!(%node, lease, "lease ended");
        if let Some(device) = self.leases.devices.get_mut(&node) {
            device.active.retain(|l| l.id() != lease);
        }
    }
}

delegate_drm_lease!(State);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_takes_only_connectors_offered_and_free() {
        let offered = [(7, false), (8, true)];
        assert_eq!(plan(&[7], &offered), Ok(vec![0]));
        // Leased by another client.
        assert_eq!(plan(&[8], &offered), Err(Refusal::Leased));
        // A screen of the desktop is never offered.
        assert_eq!(plan(&[3], &offered), Err(Refusal::NotOffered));
        assert_eq!(plan(&[7, 3], &offered), Err(Refusal::NotOffered));
    }
}
