use criterion::{criterion_group, criterion_main, Criterion};
use ruzstd::decoding::frame_decoder::FrameDecoder;

fn criterion_benchmark(c: &mut Criterion) {
    let mut fr = FrameDecoder::new();
    let mut target_slice = &mut vec![0u8; 1024 * 1024 * 200];
    let src = include_bytes!("../decodecorpus_files/z000033.zst");

    c.bench_function("decode_all_slice", |b| {
        b.iter(|| {
            fr.decode_all(src, &mut target_slice).unwrap();
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
