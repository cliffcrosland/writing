use wasm_bindgen::prelude::*;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
#[derive(Debug)]
pub struct Counter {
    key: String,
    count: i32,
}

#[wasm_bindgen]
impl Counter {
    pub fn new(key: String, count: i32) -> Counter {
        Counter { key, count }
    }

    pub fn key(&self) -> String {
        self.key.clone()
    }

    pub fn count(&self) -> i32 {
        self.count
    }

    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn update_key(&mut self, key: String) {
        self.key = key;
    }
}
