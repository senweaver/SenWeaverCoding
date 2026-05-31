use criterion::{Criterion, criterion_group, criterion_main};
use senweavercoding::agent::scheduler::{SchedulableTask, TaskScheduler};
use std::hint::black_box;

fn bench_independent_tasks(c: &mut Criterion) {
    c.bench_function("scheduler_100_independent", |b| {
        b.iter(|| {
            let mut sched = TaskScheduler::new(8);
            let tasks: Vec<SchedulableTask> = (0..100)
                .map(|i| SchedulableTask::new(format!("t{i}"), format!("Task {i}"), "prompt"))
                .collect();
            sched.add_tasks(tasks).unwrap();

            while let Some(t) = sched.claim_next() {
                sched.complete(&t.id, black_box("done".into()));
            }
            assert!(sched.is_finished());
        })
    });
}

fn bench_chain_tasks(c: &mut Criterion) {
    c.bench_function("scheduler_50_chain", |b| {
        b.iter(|| {
            let mut sched = TaskScheduler::new(4);
            let mut tasks = vec![SchedulableTask::new("t0", "Task 0", "prompt")];
            for i in 1..50 {
                tasks.push(
                    SchedulableTask::new(format!("t{i}"), format!("Task {i}"), "prompt")
                        .with_dependency(format!("t{}", i - 1)),
                );
            }
            sched.add_tasks(tasks).unwrap();

            while let Some(t) = sched.claim_next() {
                sched.complete(&t.id, black_box("done".into()));
            }
            assert!(sched.is_finished());
        })
    });
}

criterion_group!(benches, bench_independent_tasks, bench_chain_tasks);
criterion_main!(benches);
