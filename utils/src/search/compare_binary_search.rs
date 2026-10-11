use std::{thread::sleep, time::Duration};

use crate::{
    performance::{Action, ActionAgent, Tester},
    search::{binary_search, binary_search_v2, search_type::SearchContext},
};

static VECTOR_SIZE: usize = 1000;
static LOOP_COUNT: usize = 100;
static SLEEP_MICRO: u64 = 1;

#[derive(Clone, Copy)]
struct ComplexStruct {
    pub index: i32,
    _some_data: [f32; 4],
    _some_data_2: [f32; 4],
}
impl ComplexStruct {
    pub fn get(&self) -> &Self {
        // simulate the fetch case (reading file, api call)
        sleep(Duration::from_micros(SLEEP_MICRO));
        self
    }
}

impl Into<ComplexStruct> for i32 {
    fn into(self) -> ComplexStruct {
        ComplexStruct {
            index: self,
            _some_data: [1f32, 2f32, 3f32, 4f32],
            _some_data_2: [5f32, 6f32, 7f32, 8f32],
        }
    }
}

impl PartialEq<i32> for ComplexStruct {
    fn eq(&self, other: &i32) -> bool {
        &self.get().index == other
    }
}
impl PartialOrd<i32> for ComplexStruct {
    fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
        self.get().index.partial_cmp(other)
    }
}

#[derive(Clone)]
struct BaseAction {
    data: Vec<ComplexStruct>,
    pub query: i32,
}

impl BaseAction {
    pub fn new(size: usize, query: i32) -> Self {
        let mut data = Vec::with_capacity(size);
        for i in 0..size {
            data.push((i as i32).into());
        }
        Self { data, query }
    }
}

struct ActionV1(pub BaseAction);

impl Action for ActionV1 {
    fn execute(&mut self) {
        binary_search::dicotomic_search(
            &self.0.data,
            &self.0.query,
            binary_search::SearchQuery::Any,
        );
    }
}

struct ActionV2(pub BaseAction);

impl Action for ActionV2 {
    fn execute(&mut self) {
        binary_search_v2::dicotomic_search(&self.0.data, &self.0.query);
    }
}

struct ActionV2Context {
    base: BaseAction,
    context: SearchContext,
    base_context: SearchContext,
}
impl ActionV2Context {
    pub fn new(base: BaseAction) -> Self {
        Self {
            base,
            context: SearchContext::none(),
            base_context: SearchContext::none(),
        }
    }
    pub fn new_with_context(base: BaseAction, context: SearchContext) -> Self {
        Self {
            base,
            context: context.clone(),
            base_context: context,
        }
    }
}

impl Action for ActionV2Context {
    fn init(&mut self) {
        self.context = self.base_context.clone();
    }
    fn execute(&mut self) {
        binary_search_v2::dicotomic_search_with_context(
            &self.base.data,
            &self.base.query,
            &mut self.context,
        );
    }
}

struct ActionV1All(pub BaseAction);
impl Action for ActionV1All {
    fn execute(&mut self) {
        for i in &self.0.data {
            binary_search::dicotomic_search(
                &self.0.data,
                &i.index,
                binary_search::SearchQuery::Any,
            );
        }
    }
}
struct ActionV2All(pub BaseAction);
impl Action for ActionV2All {
    fn execute(&mut self) {
        for i in &self.0.data {
            binary_search_v2::dicotomic_search(&self.0.data, &i.index);
        }
    }
}

pub fn compare_search() {
    println!("Compare search");
    let mut tester = Tester::new(LOOP_COUNT);

    let check = BaseAction::new(VECTOR_SIZE, 1);

    let check_v1 = ActionV1(check.clone());
    let check_v2 = ActionV2(check.clone());
    let check_v2_context = ActionV2Context::new(check.clone());
    let check_v2_context_init =
        ActionV2Context::new_with_context(check.clone(), SearchContext::of(1));

    tester.add(ActionAgent::new("v1", Box::new(check_v1)));
    tester.add(ActionAgent::new("v2", Box::new(check_v2)));
    tester.add(ActionAgent::new("v2_context", Box::new(check_v2_context)));
    tester.add(ActionAgent::new(
        "check_v2_context_init",
        Box::new(check_v2_context_init),
    ));

    tester.execute();

    println!("{}", tester);
}

pub fn compare_search_middle() {
    println!("Compare search middle");
    let mut tester = Tester::new(LOOP_COUNT);

    let check = BaseAction::new(VECTOR_SIZE, (VECTOR_SIZE / 2) as i32);

    let check_v1 = ActionV1(check.clone());
    let check_v2 = ActionV2(check.clone());
    let check_v2_context = ActionV2Context::new(check.clone());
    let check_v2_context_init =
        ActionV2Context::new_with_context(check.clone(), SearchContext::of(VECTOR_SIZE / 4));

    tester.add(ActionAgent::new("v1", Box::new(check_v1)));
    tester.add(ActionAgent::new("v2", Box::new(check_v2)));
    tester.add(ActionAgent::new("v2_context", Box::new(check_v2_context)));
    tester.add(ActionAgent::new(
        "check_v2_context_init",
        Box::new(check_v2_context_init),
    ));

    tester.execute();

    println!("{}", tester);
}

pub fn compare_search_out() {
    println!("Compare search out lower");

    let mut tester = Tester::new(LOOP_COUNT);

    let check = BaseAction::new(VECTOR_SIZE, -1);

    let check_v1 = ActionV1(check.clone());
    let check_v2 = ActionV2(check.clone());
    let check_v2_context = ActionV2Context::new(check.clone());
    let check_v2_context_init =
        ActionV2Context::new_with_context(check.clone(), SearchContext::of(1));

    tester.add(ActionAgent::new("v1", Box::new(check_v1)));
    tester.add(ActionAgent::new("v2", Box::new(check_v2)));
    tester.add(ActionAgent::new("v2_context", Box::new(check_v2_context)));
    tester.add(ActionAgent::new(
        "check_v2_context_init",
        Box::new(check_v2_context_init),
    ));

    tester.execute();

    println!("{}", tester);
}

pub fn compare_search_out_upper() {
    println!("Compare search out upper");

    let mut tester = Tester::new(LOOP_COUNT);

    let check = BaseAction::new(VECTOR_SIZE, VECTOR_SIZE as i32);

    let check_v1 = ActionV1(check.clone());
    let check_v2 = ActionV2(check.clone());
    let check_v2_context = ActionV2Context::new(check.clone());
    let check_v2_context_init =
        ActionV2Context::new_with_context(check.clone(), SearchContext::of(1));

    tester.add(ActionAgent::new("v1", Box::new(check_v1)));
    tester.add(ActionAgent::new("v2", Box::new(check_v2)));
    tester.add(ActionAgent::new("v2_context", Box::new(check_v2_context)));
    tester.add(ActionAgent::new(
        "check_v2_context_init",
        Box::new(check_v2_context_init),
    ));

    tester.execute();

    println!("{}", tester);
}

pub fn compare_search_single() {
    println!("Compare search single");

    let mut tester = Tester::new(LOOP_COUNT);

    let check = BaseAction::new(1, 2);

    let check_v1 = ActionV1(check.clone());
    let check_v2 = ActionV2(check.clone());
    let check_v2_context = ActionV2Context::new(check.clone());
    let check_v2_context_init =
        ActionV2Context::new_with_context(check.clone(), SearchContext::of(1));

    tester.add(ActionAgent::new("v1", Box::new(check_v1)));
    tester.add(ActionAgent::new("v2", Box::new(check_v2)));
    tester.add(ActionAgent::new("v2_context", Box::new(check_v2_context)));
    tester.add(ActionAgent::new(
        "check_v2_context_init",
        Box::new(check_v2_context_init),
    ));

    tester.execute();

    println!("{}", tester);
}

pub fn compare_search_all() {
    println!("Compare search all");

    let mut tester = Tester::new(LOOP_COUNT);

    let check = BaseAction::new(VECTOR_SIZE, -1);

    let check_v1 = ActionV1All(check.clone());
    let check_v2 = ActionV2All(check.clone());
    let check_v2_context = ActionV2Context::new(check.clone());
    let check_v2_context_init =
        ActionV2Context::new_with_context(check.clone(), SearchContext::of(1));

    tester.add(ActionAgent::new("v1", Box::new(check_v1)));
    tester.add(ActionAgent::new("v2", Box::new(check_v2)));
    tester.add(ActionAgent::new("v2_context", Box::new(check_v2_context)));
    tester.add(ActionAgent::new(
        "check_v2_context_init",
        Box::new(check_v2_context_init),
    ));

    tester.execute();

    println!("{}", tester);
}

pub fn compare_search_final(all_only: bool) {
    if !all_only {
        compare_search();

        compare_search_middle();

        compare_search_out();

        compare_search_out_upper();

        compare_search_single();
    } else {
        compare_search_all();
    }
}
