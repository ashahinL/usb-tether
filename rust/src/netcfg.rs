//! Plan the two /1 routes and a temporary DNS service.
//! Callers pass plain values. This module does not change routes or DNS on the Mac.
//! `install`, `rollback`, and `startup_sweep` talk to a `RouteStore`.
//! `MemoryRoutes` and `MemoryDns` keep those changes in memory.

use std::net::Ipv4Addr;

/// What the caller observed when it tried to add one route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    Succeeded,
    Eexist,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub destination: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub interface_index: u32,
}

/// Previous process id read from the pidfile. This module does not signal that process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pidfile {
    pub pid: u32,
}

/// The only routes a session may add: `0.0.0.0/1` and `128.0.0.0/1`.
/// Neither entry is `0.0.0.0/0`.
pub fn planned_routes(gateway: Ipv4Addr, interface_index: u32) -> [Route; 2] {
    [
        Route {
            destination: Ipv4Addr::new(0, 0, 0, 0),
            mask: Ipv4Addr::new(128, 0, 0, 0),
            gateway,
            interface_index,
        },
        Route {
            destination: Ipv4Addr::new(128, 0, 0, 0),
            mask: Ipv4Addr::new(128, 0, 0, 0),
            gateway,
            interface_index,
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Session {
    recorded: Vec<Route>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            recorded: Vec::new(),
        }
    }

    pub fn recorded(&self) -> &[Route] {
        &self.recorded
    }

    /// Keep `route` only when the caller reports that the add succeeded.
    /// `Eexist` and `Failed` are not recorded, so a later cleanup must not delete them.
    pub fn record_added(&mut self, route: Route, outcome: AddOutcome) {
        if outcome == AddOutcome::Succeeded {
            self.recorded.push(route);
        }
    }

    /// Recorded routes that still match `current` on destination, mask, gateway, and interface index.
    pub fn cleanup(&self, current: &[Route]) -> Vec<Route> {
        self.recorded
            .iter()
            .copied()
            .filter(|route| current.contains(route))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsPlan {
    pub service_id: String,
    pub servers: Vec<Ipv4Addr>,
    pub keys: [String; 3],
}

impl DnsPlan {
    pub fn new(service_id: impl Into<String>, servers: Vec<Ipv4Addr>) -> Self {
        let service_id = service_id.into();
        let prefix = format!("State:/Network/Service/{service_id}");
        Self {
            keys: [
                format!("{prefix}/IPv4"),
                format!("{prefix}/DNS"),
                format!("{prefix}/Interface"),
            ],
            service_id,
            servers,
        }
    }
}

/// Routes to delete because `pidfile.pid` is not alive and a saved record exists.
/// A live process yields nothing, even when `saved` is present.
pub fn startup_cleanup(pidfile: &Pidfile, alive: bool, saved: Option<&[Route]>) -> Vec<Route> {
    // The caller already decided whether this pid is alive. Do not signal it.
    let Pidfile { pid: _ } = *pidfile;
    if alive {
        return Vec::new();
    }
    saved.unwrap_or(&[]).to_vec()
}

const GLOBAL_DNS_KEY: &str = "State:/Network/Global/DNS";

pub trait RouteStore {
    fn add(&mut self, route: &Route) -> AddOutcome;
    fn current(&self) -> Vec<Route>;
    fn delete(&mut self, route: &Route) -> bool;
}

/// In-memory routes. `add` returns `EEXIST` when the same route is already stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryRoutes {
    routes: Vec<Route>,
}

impl MemoryRoutes {
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }
}

impl RouteStore for MemoryRoutes {
    fn add(&mut self, route: &Route) -> AddOutcome {
        if self.routes.contains(route) {
            return AddOutcome::Eexist;
        }
        self.routes.push(*route);
        AddOutcome::Succeeded
    }

    fn current(&self) -> Vec<Route> {
        self.routes.clone()
    }

    fn delete(&mut self, route: &Route) -> bool {
        let before = self.routes.len();
        self.routes.retain(|existing| existing != route);
        self.routes.len() != before
    }
}

/// Add both planned `/1` routes. `EEXIST` is not recorded.
pub fn install(
    store: &mut impl RouteStore,
    session: &mut Session,
    gateway: Ipv4Addr,
    interface_index: u32,
) {
    for route in planned_routes(gateway, interface_index) {
        let outcome = store.add(&route);
        session.record_added(route, outcome);
    }
}

/// Delete recorded routes that are still present. Every other route stays.
pub fn rollback(store: &mut impl RouteStore, session: &Session) {
    let current = store.current();
    for route in session.cleanup(&current) {
        store.delete(&route);
    }
}

/// When `alive` is false, delete saved routes that are still in the store.
/// A live pid returns without deleting. This does not signal `pidfile.pid`.
pub fn startup_sweep(
    store: &mut impl RouteStore,
    pidfile: &Pidfile,
    alive: bool,
    saved: Option<&[Route]>,
) {
    if alive {
        return;
    }
    let current = store.current();
    for route in startup_cleanup(pidfile, alive, saved) {
        if current.contains(&route) {
            store.delete(&route);
        }
    }
}

/// Publish and clear one temporary service. Never record or delete `State:/Network/Global/DNS`.
pub trait DnsPublisher {
    fn publish(&mut self, plan: &DnsPlan);
    fn clear(&mut self, service_id: &str);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedKey {
    service_id: String,
    key: String,
}

/// In-memory DNS keys. `clear` drops only the given service id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryDns {
    recorded: Vec<RecordedKey>,
}

impl MemoryDns {
    pub fn new() -> Self {
        Self {
            recorded: Vec::new(),
        }
    }

    pub fn keys(&self) -> Vec<String> {
        self.recorded.iter().map(|item| item.key.clone()).collect()
    }
}

impl DnsPublisher for MemoryDns {
    fn publish(&mut self, plan: &DnsPlan) {
        self.recorded
            .retain(|item| item.key == GLOBAL_DNS_KEY || item.service_id != plan.service_id);
        for key in &plan.keys {
            if key == GLOBAL_DNS_KEY {
                continue;
            }
            self.recorded.push(RecordedKey {
                service_id: plan.service_id.clone(),
                key: key.clone(),
            });
        }
    }

    fn clear(&mut self, service_id: &str) {
        self.recorded
            .retain(|item| item.key == GLOBAL_DNS_KEY || item.service_id != service_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(destination: [u8; 4], mask: [u8; 4], gateway: [u8; 4], interface_index: u32) -> Route {
        Route {
            destination: Ipv4Addr::new(
                destination[0],
                destination[1],
                destination[2],
                destination[3],
            ),
            mask: Ipv4Addr::new(mask[0], mask[1], mask[2], mask[3]),
            gateway: Ipv4Addr::new(gateway[0], gateway[1], gateway[2], gateway[3]),
            interface_index,
        }
    }

    #[test]
    fn plans_two_slash_one_routes_and_not_the_default() {
        let gateway = Ipv4Addr::new(192, 168, 42, 129);
        let routes = planned_routes(gateway, 12);
        let default_route = route([0, 0, 0, 0], [0, 0, 0, 0], [192, 168, 42, 129], 12);

        assert_eq!(
            routes,
            [
                route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12),
                route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12),
            ]
        );
        assert!(routes.iter().all(|planned| *planned != default_route));
    }

    #[test]
    fn record_added_keeps_success_and_skips_eexist() {
        let routes = planned_routes(Ipv4Addr::new(192, 168, 42, 129), 4);
        let mut session = Session::new();

        assert_eq!(session.recorded(), &[] as &[Route]);
        session.record_added(routes[0], AddOutcome::Eexist);
        assert_eq!(session.recorded(), &[] as &[Route]);

        session.record_added(routes[0], AddOutcome::Succeeded);
        session.record_added(routes[1], AddOutcome::Eexist);
        assert_eq!(session.recorded(), &[routes[0]]);
    }

    #[test]
    fn cleanup_returns_only_recorded_routes_still_present() {
        let low = route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 7);
        let high = route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 7);
        let foreign = route([10, 0, 0, 0], [255, 0, 0, 0], [10, 0, 0, 1], 1);
        let mut other_gateway = high;
        other_gateway.gateway = Ipv4Addr::new(1, 2, 3, 4);
        let mut other_mask = low;
        other_mask.mask = Ipv4Addr::new(255, 255, 255, 0);
        let mut other_index = low;
        other_index.interface_index = 9;

        let mut session = Session::new();
        session.record_added(low, AddOutcome::Succeeded);
        session.record_added(high, AddOutcome::Succeeded);

        assert_eq!(session.cleanup(&[low, high, foreign]), vec![low, high]);
        assert_eq!(session.cleanup(&[low, foreign]), vec![low]);
        assert_eq!(session.cleanup(&[foreign]), Vec::<Route>::new());
        assert_eq!(session.cleanup(&[other_gateway]), Vec::<Route>::new());
        assert_eq!(session.cleanup(&[other_mask]), Vec::<Route>::new());
        assert_eq!(session.cleanup(&[other_index]), Vec::<Route>::new());
    }

    #[test]
    fn dns_plan_uses_service_keys_and_not_global_dns() {
        let plan = DnsPlan::new(
            "tether-9f3a",
            vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)],
        );

        assert_eq!(plan.service_id, "tether-9f3a");
        assert_eq!(
            plan.servers,
            vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)]
        );
        assert_eq!(
            plan.keys,
            [
                "State:/Network/Service/tether-9f3a/IPv4",
                "State:/Network/Service/tether-9f3a/DNS",
                "State:/Network/Service/tether-9f3a/Interface",
            ]
        );
        assert!(plan
            .keys
            .iter()
            .all(|key| key != "State:/Network/Global/DNS"));
    }

    #[test]
    fn startup_cleanup_returns_saved_routes_only_when_the_previous_process_is_dead() {
        let pidfile = Pidfile { pid: 4242 };
        let saved = planned_routes(Ipv4Addr::new(192, 168, 42, 129), 3);

        assert_eq!(pidfile.pid, 4242);
        assert_eq!(
            startup_cleanup(&pidfile, true, Some(&saved)),
            Vec::<Route>::new()
        );
        assert_eq!(
            startup_cleanup(&pidfile, false, Some(&saved)),
            saved.to_vec()
        );
        assert_eq!(startup_cleanup(&pidfile, false, None), Vec::<Route>::new());
    }

    #[test]
    fn install_records_both_routes_on_success() {
        let mut store = MemoryRoutes::new();
        let mut session = Session::new();

        install(
            &mut store,
            &mut session,
            Ipv4Addr::new(192, 168, 42, 129),
            12,
        );

        let expected = [
            route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12),
            route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12),
        ];
        assert_eq!(session.recorded(), &expected);
        assert_eq!(store.current(), expected.to_vec());
    }

    #[test]
    fn eexist_route_is_kept_on_rollback() {
        let low = route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12);
        let high = route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12);
        let mut store = MemoryRoutes::new();
        assert_eq!(store.add(&low), AddOutcome::Succeeded);
        let mut session = Session::new();
        install(
            &mut store,
            &mut session,
            Ipv4Addr::new(192, 168, 42, 129),
            12,
        );

        assert_eq!(session.recorded(), &[high]);
        rollback(&mut store, &session);
        assert_eq!(store.current(), vec![low]);
    }

    #[test]
    fn rollback_leaves_an_unrelated_route() {
        let foreign = route([10, 0, 0, 0], [255, 0, 0, 0], [10, 0, 0, 1], 1);
        let low = route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12);
        let high = route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 12);
        let mut store = MemoryRoutes::new();
        assert_eq!(store.add(&foreign), AddOutcome::Succeeded);
        let mut session = Session::new();
        install(
            &mut store,
            &mut session,
            Ipv4Addr::new(192, 168, 42, 129),
            12,
        );

        assert_eq!(session.recorded(), &[low, high]);
        assert_eq!(store.current(), vec![foreign, low, high]);
        rollback(&mut store, &session);
        assert_eq!(store.current(), vec![foreign]);
    }

    #[test]
    fn dns_clear_removes_only_our_service_id() {
        let mut dns = MemoryDns::new();
        let mut ours = DnsPlan::new("tether-9f3a", vec![Ipv4Addr::new(8, 8, 8, 8)]);
        ours.keys[1] = "State:/Network/Global/DNS".to_string();
        let other = DnsPlan::new("phone-dns", vec![Ipv4Addr::new(1, 1, 1, 1)]);

        dns.publish(&ours);
        dns.publish(&other);
        dns.recorded.push(RecordedKey {
            service_id: "tether-9f3a".to_string(),
            key: "State:/Network/Global/DNS".to_string(),
        });

        assert_eq!(
            dns.keys(),
            vec![
                "State:/Network/Service/tether-9f3a/IPv4".to_string(),
                "State:/Network/Service/tether-9f3a/Interface".to_string(),
                "State:/Network/Service/phone-dns/IPv4".to_string(),
                "State:/Network/Service/phone-dns/DNS".to_string(),
                "State:/Network/Service/phone-dns/Interface".to_string(),
                "State:/Network/Global/DNS".to_string(),
            ]
        );

        dns.clear("tether-9f3a");

        assert_eq!(
            dns.keys(),
            vec![
                "State:/Network/Service/phone-dns/IPv4".to_string(),
                "State:/Network/Service/phone-dns/DNS".to_string(),
                "State:/Network/Service/phone-dns/Interface".to_string(),
                "State:/Network/Global/DNS".to_string(),
            ]
        );
    }

    fn sweep_fixtures() -> (MemoryRoutes, [Route; 2]) {
        let saved = [
            route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 3),
            route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 3),
        ];
        let different_index = route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 9);
        let foreign = route([10, 0, 0, 0], [255, 0, 0, 0], [10, 0, 0, 1], 1);
        let mut store = MemoryRoutes::new();
        assert_eq!(store.add(&saved[0]), AddOutcome::Succeeded);
        assert_eq!(store.add(&saved[1]), AddOutcome::Succeeded);
        assert_eq!(store.add(&different_index), AddOutcome::Succeeded);
        assert_eq!(store.add(&foreign), AddOutcome::Succeeded);
        (store, saved)
    }

    #[test]
    fn live_pidfile_deletes_nothing() {
        let (mut store, saved) = sweep_fixtures();

        startup_sweep(&mut store, &Pidfile { pid: 4242 }, true, Some(&saved));

        assert_eq!(
            store.current(),
            vec![
                route([0, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 3),
                route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 3),
                route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 9),
                route([10, 0, 0, 0], [255, 0, 0, 0], [10, 0, 0, 1], 1),
            ]
        );
    }

    #[test]
    fn dead_pidfile_deletes_saved_routes_still_present() {
        let (mut store, saved) = sweep_fixtures();

        startup_sweep(&mut store, &Pidfile { pid: 4242 }, false, Some(&saved));

        assert_eq!(
            store.current(),
            vec![
                route([128, 0, 0, 0], [128, 0, 0, 0], [192, 168, 42, 129], 9),
                route([10, 0, 0, 0], [255, 0, 0, 0], [10, 0, 0, 1], 1),
            ]
        );
    }
}
