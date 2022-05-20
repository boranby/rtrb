#[path = "../../benches/two_threads.rs"]
mod two_threads;

use two_threads::add_function;

use criterion::{criterion_group, criterion_main};

fn criterion_benchmark(criterion: &mut criterion::Criterion) {
    let mut group = criterion.benchmark_group("two-threads");

    add_function(
        &mut group,
        "-concurrent-queue",
        |capacity| {
            let q = std::sync::Arc::new(concurrent_queue::ConcurrentQueue::bounded(capacity));
            (q.clone(), q)
        },
        |q, i| q.push(i).is_ok(),
        |q| q.pop().ok(),
    );

    add_function(
        &mut group,
        "-crossbeam-queue",
        |capacity| {
            let q = std::sync::Arc::new(crossbeam_queue::ArrayQueue::new(capacity));
            (q.clone(), q)
        },
        |q, i| q.push(i).is_ok(),
        |q| q.pop(),
    );

    add_function(
        &mut group,
        "-crossbeam-queue-pr338",
        crossbeam_queue_pr338::spsc::new,
        |q, i| q.push(i).is_ok(),
        |q| q.pop().ok(),
    );

    use magnetic::{Consumer, Producer};

    add_function(
        &mut group,
        "-magnetic",
        |capacity| {
            let buffer = magnetic::buffer::dynamic::DynamicBuffer::new(capacity).unwrap();
            magnetic::spsc::spsc_queue(buffer)
        },
        |p, i| p.try_push(i).is_ok(),
        |c| c.try_pop().ok(),
    );

    add_function(
        &mut group,
        "-npnc",
        |capacity| npnc::bounded::spsc::channel(capacity.next_power_of_two()),
        |p, i| p.produce(i).is_ok(),
        |c| c.consume().ok(),
    );

    add_function(
        &mut group,
        "-ringbuf",
        |capacity| ringbuf::RingBuffer::new(capacity).split(),
        |p, i| p.push(i).is_ok(),
        |c| c.pop(),
    );

    add_function(
        &mut group,
        "-rtrb",
        rtrb::RingBuffer::new,
        |p, i| p.push(i).is_ok(),
        |c| c.pop().ok(),
    );

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
