use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use std::net::SocketAddr;
use tpt_frontier_proxy::loadbalancer::{
    BackendEndpoint, ConsistentHashBalancer, LeastConnectionsBalancer, LoadBalancer,
    RoundRobinBalancer,
};

fn make_endpoints(n: usize) -> Vec<BackendEndpoint> {
    (0..n)
        .map(|i| BackendEndpoint {
            addr: SocketAddr::from(([127, 0, 0, 1], 8080 + i as u16)),
            weight: 1,
            tags: HashMap::new(),
        })
        .collect()
}

fn bench_round_robin(c: &mut Criterion) {
    let endpoints = make_endpoints(4);
    let balancer = RoundRobinBalancer::new(endpoints);

    c.bench_function("round_robin_next_endpoint", |b| {
        b.iter(|| black_box(balancer.next_endpoint()))
    });
}

fn bench_round_robin_16(c: &mut Criterion) {
    let endpoints = make_endpoints(16);
    let balancer = RoundRobinBalancer::new(endpoints);

    c.bench_function("round_robin_16_endpoints", |b| {
        b.iter(|| black_box(balancer.next_endpoint()))
    });
}

fn bench_least_connections(c: &mut Criterion) {
    let endpoints = make_endpoints(4);
    let balancer = LeastConnectionsBalancer::new(endpoints);

    c.bench_function("least_connections_next_endpoint", |b| {
        b.iter(|| black_box(balancer.next_endpoint()))
    });
}

fn bench_consistent_hash(c: &mut Criterion) {
    let endpoints = make_endpoints(4);
    let balancer = ConsistentHashBalancer::new(endpoints, 150);

    c.bench_function("consistent_hash_next_endpoint", |b| {
        b.iter(|| black_box(balancer.next_endpoint()))
    });
}

fn bench_consistent_hash_16(c: &mut Criterion) {
    let endpoints = make_endpoints(16);
    let balancer = ConsistentHashBalancer::new(endpoints, 150);

    c.bench_function("consistent_hash_16_endpoints", |b| {
        b.iter(|| black_box(balancer.next_endpoint()))
    });
}

fn bench_mark_healthy_cycle(c: &mut Criterion) {
    let endpoints = make_endpoints(4);
    let balancer = RoundRobinBalancer::new(endpoints.clone());

    c.bench_function("mark_healthy_cycle", |b| {
        b.iter(|| {
            for ep in &endpoints {
                black_box(balancer.mark_healthy(ep));
            }
            black_box(balancer.next_endpoint())
        })
    });
}

criterion_group!(
    benches,
    bench_round_robin,
    bench_round_robin_16,
    bench_least_connections,
    bench_consistent_hash,
    bench_consistent_hash_16,
    bench_mark_healthy_cycle,
);
criterion_main!(benches);
