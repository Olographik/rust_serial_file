use core::fmt;
use std::time::{Duration, SystemTime, SystemTimeError};

use rand::{RngExt, SeedableRng, rngs::StdRng};

pub struct PerfomanceMeasure {
    start: SystemTime,
    end: SystemTime,
}

impl PerfomanceMeasure {
    pub fn start() -> Self {
        let start = SystemTime::now();
        Self { start, end: start }
    }

    pub fn restart(&mut self) {
        self.start = SystemTime::now();
        self.end = self.start;
    }

    pub fn end(&mut self) {
        self.end = SystemTime::now();
    }

    pub fn duration(&self) -> Result<Duration, SystemTimeError> {
        self.end.duration_since(self.start)
    }
}
impl fmt::Display for PerfomanceMeasure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(v) = self.duration() {
            write!(f, "{:?}", v)
        } else {
            write!(f, "An Error occur in the time delta")
        }
    }
}

pub struct AveragePerformance {
    pub durations: Vec<Duration>,
    current: PerfomanceMeasure,
    has_current: bool,
}

impl AveragePerformance {
    pub fn new() -> Self {
        AveragePerformance {
            durations: Vec::new(),
            current: PerfomanceMeasure::start(),
            has_current: false,
        }
    }

    pub fn add(&mut self, performance: &PerfomanceMeasure) {
        match performance.duration() {
            Ok(d) => self.durations.push(d),
            Err(_) => {}
        }
    }

    pub fn start_new(&mut self) {
        self.end_current();

        self.current.restart();
        self.has_current = true;
    }

    pub fn end_current(&mut self) {
        if self.has_current {
            self.current.end();

            match self.current.duration() {
                Ok(d) => self.durations.push(d),
                Err(_) => {}
            };

            self.has_current = false;
        }
    }

    pub fn restart(&mut self) {
        self.has_current = false;
        self.durations.clear();
    }

    pub fn average(&self) -> Duration {
        let mut duration_avg = Duration::from_nanos(0);
        for d in &self.durations {
            duration_avg += *d;
        }
        duration_avg / (self.durations.len() as u32)
    }

    pub fn total(&self) -> Duration {
        self.durations.iter().sum()
    }
}

impl fmt::Display for AveragePerformance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} for {}", self.average(), self.durations.len())
    }
}

trait Action {
    fn init(&mut self) {
        // initialize the action, set the context
    }

    // The action execution, the point that need to be measured
    fn execute(&mut self);

    fn clean(&mut self) {
        // clean the context, file or other component
    }
}

impl<F: FnMut()> Action for F {
    fn execute(&mut self) {
        self();
    }
}

struct ActionAgent {
    pub measure: AveragePerformance,
    pub action: Box<dyn Action>,
    pub agent_name: String,
}

macro_rules! action_agent {
    ($action:path) => {
        $crate::performance::ActionAgent::new(stringify!($action), Box::new($action))
    };
}

impl ActionAgent {
    pub fn new(agent_name: impl Into<String>, action: Box<dyn Action>) -> Self {
        Self {
            measure: AveragePerformance::new(),
            action,
            agent_name: agent_name.into(),
        }
    }

    pub fn count(&self) -> usize {
        self.measure.durations.len()
    }

    pub fn execute(&mut self) {
        self.action.init();

        self.measure.start_new();

        self.action.execute();

        self.measure.end_current();

        self.action.clean();
    }

    pub fn average(&self) -> Duration {
        self.measure.average()
    }
}

impl fmt::Display for ActionAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {:?} for {} loop and a total of {:?}",
            self.agent_name,
            self.average(),
            self.measure.durations.len(),
            self.measure.total()
        )
    }
}

struct Tester {
    pub loop_count: usize,
    pub actions: Vec<ActionAgent>,
}

impl Tester {
    pub fn new(loop_count: usize) -> Self {
        Self {
            loop_count,
            actions: Vec::new(),
        }
    }

    pub fn add(&mut self, action: ActionAgent) -> &mut Self {
        self.actions.push(action);
        self
    }

    pub fn execute_one_loop<T: rand::Rng>(actions: &mut Vec<&mut ActionAgent>, rand: &mut T) {
        while !actions.is_empty() {
            let idx = rand.random_range(0..actions.len());
            actions[idx].execute();
            actions.remove(idx);
        }
    }

    pub fn execute_with_rand<T: rand::Rng>(&mut self, rand: &mut T) {
        let mut can_execute = !self.actions.is_empty();

        while can_execute {
            let mut actions_to_execute: Vec<&mut ActionAgent> = self
                .actions
                .iter_mut()
                .filter(|a| a.count() < self.loop_count)
                .collect();

            can_execute = !actions_to_execute.is_empty();
            if can_execute {
                Tester::execute_one_loop(&mut actions_to_execute, rand);
            }
        }
    }

    pub fn execute(&mut self) {
        let mut rand = rand::rng();
        self.execute_with_rand(&mut rand);
    }

    pub fn execute_seeded(&mut self, seed: u64) {
        let mut rand = StdRng::seed_from_u64(seed);
        self.execute_with_rand(&mut rand);
    }
}

impl fmt::Display for Tester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for agent in self.actions.iter() {
            write!(f, "{}\n", agent)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    use crate::performance::Tester;

    fn sleep_10ms() {
        sleep(Duration::from_millis(10));
    }

    fn sleep_100ms() {
        sleep(Duration::from_millis(100));
    }

    #[test]
    fn test_simple_performance() {
        let mut tester = Tester::new(3);
        tester.add(action_agent!(sleep_10ms));
        tester.add(action_agent!(sleep_100ms));
        assert_eq!(tester.actions[0].agent_name, "sleep_10ms");
        assert_eq!(tester.actions[1].agent_name, "sleep_100ms");

        tester.execute();

        assert_eq!(3, tester.actions[0].count());

        assert_eq!(10, tester.actions[0].average().as_millis());
        let total = tester.actions[0].measure.total().as_millis();
        assert!(30 <= total && total <= 31);

        assert_eq!(3, tester.actions[1].count());

        assert_eq!(100, tester.actions[1].average().as_millis());
        let total = tester.actions[1].measure.total().as_millis();
        assert!(300 <= total && total <= 301);

        println!("{}", tester);
    }
}
