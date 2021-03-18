use crate::map::Map;
use quickcheck::{Arbitrary, Gen, QuickCheck, TestResult};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
enum Operation {
    Insert(u16, u8),
    Remove(u16),
    Get(u16),
    GetMut(u16),
    GetLen,
    GetIsEmpty,
    Clear,
    ContainsKey(u16),
    Iter,
    IterMut,
    Keys,
    Values,
}

impl Arbitrary for Operation {
    fn arbitrary(gen: &mut Gen) -> Self {
        let variant: u8 = u8::arbitrary(gen);
        match variant % 12 {
            0 => Operation::Insert(u16::arbitrary(gen), u8::arbitrary(gen)),
            1 => Operation::Remove(u16::arbitrary(gen)),
            2 => Operation::Get(u16::arbitrary(gen)),
            3 => Operation::GetLen,
            4 => Operation::GetIsEmpty,
            5 => Operation::Clear,
            6 => Operation::ContainsKey(u16::arbitrary(gen)),
            7 => Operation::GetMut(u16::arbitrary(gen)),
            8 => Operation::Iter,
            9 => Operation::IterMut,
            10 => Operation::Keys,
            11 => Operation::Values,
            _ => unreachable!(),
        }
    }
}

#[test]
fn model_check() {
    fn inner(input: Vec<Operation>) -> TestResult {
        let mut model: BTreeMap<u16, u8> = BTreeMap::new();
        let mut sut: Map<u16, u8> = Map::new();

        for op in &input {
            match op {
                Operation::Insert(k, v) => assert_eq!(model.insert(*k, *v), sut.insert(*k, *v)),
                Operation::Remove(k) => assert_eq!(model.remove(k), sut.remove(k)),
                Operation::Get(k) => assert_eq!(model.get(k), sut.get(k)),
                Operation::GetLen => assert_eq!(model.len(), sut.len()),
                Operation::GetIsEmpty => assert_eq!(model.is_empty(), sut.is_empty()),
                Operation::Clear => assert_eq!(model.clear(), sut.clear()),
                Operation::ContainsKey(k) => assert_eq!(model.contains_key(k), sut.contains_key(k)),
                Operation::GetMut(k) => assert_eq!(model.get_mut(k), sut.get_mut(k)),
                Operation::Iter => {
                    let mut model_iter = model.iter();
                    let mut sut_iter = sut.iter();
                    while let Some(model_kv) = model_iter.next() {
                        assert_eq!(Some(model_kv), sut_iter.next());
                    }
                    assert!(sut_iter.next().is_none())
                }
                Operation::IterMut => {
                    let mut model_iter = model.iter_mut();
                    let mut sut_iter = sut.iter_mut();
                    while let Some(model_kv) = model_iter.next() {
                        assert_eq!(Some(model_kv), sut_iter.next());
                    }
                    assert!(sut_iter.next().is_none())
                }
                Operation::Keys => {
                    let mut model_iter = model.keys();
                    let mut sut_iter = sut.keys();
                    while let Some(model_key) = model_iter.next() {
                        assert_eq!(Some(model_key), sut_iter.next());
                    }
                    assert!(sut_iter.next().is_none())
                }
                Operation::Values => {
                    let mut model_iter = model.values();
                    let mut sut_iter = sut.values();
                    while let Some(model_value) = model_iter.next() {
                        assert_eq!(Some(model_value), sut_iter.next());
                    }
                    assert!(sut_iter.next().is_none())
                }
            }
        }

        TestResult::passed()
    }
    QuickCheck::new()
        .max_tests(1_000)
        .tests(500)
        .quickcheck(inner as fn(Vec<Operation>) -> TestResult);
}
