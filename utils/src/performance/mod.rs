use core::fmt;
use std::time::{Duration, SystemTime, SystemTimeError};

use crate::random::{IndexRng, SplitMix64};

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

pub trait Action {
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

pub struct ActionAgent {
    pub measure: AveragePerformance,
    pub action: Box<dyn Action>,
    pub agent_name: String,
}

#[macro_export]
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

pub struct Tester {
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

    /// Runs every action of `actions` once, in an order drawn from `rng`,
    /// and empties `actions`.
    ///
    /// An index returned out of range by `rng` is reduced modulo the number
    /// of remaining actions.
    pub fn execute_one_loop<R: IndexRng + ?Sized>(
        actions: &mut Vec<&mut ActionAgent>,
        rng: &mut R,
    ) {
        while !actions.is_empty() {
            let idx = rng.next_index(actions.len()) % actions.len();
            actions[idx].execute();
            actions.remove(idx);
        }
    }

    /// Runs every action `loop_count` times, in an order drawn from `rng`.
    ///
    /// Any [`IndexRng`] can be injected: a `rand` generator (with the `rand`
    /// feature), a [`SplitMix64`], or a closure through
    /// [`FnIndexRng`](crate::random::FnIndexRng).
    pub fn execute_with_rand<R: IndexRng + ?Sized>(&mut self, rng: &mut R) {
        let mut can_execute = !self.actions.is_empty();

        while can_execute {
            let mut actions_to_execute: Vec<&mut ActionAgent> = self
                .actions
                .iter_mut()
                .filter(|a| a.count() < self.loop_count)
                .collect();

            can_execute = !actions_to_execute.is_empty();
            if can_execute {
                Tester::execute_one_loop(&mut actions_to_execute, rng);
            }
        }
    }

    /// Runs the actions in an order drawn from the operating system's
    /// entropy (`rand::rng()`). Needs the `rand` feature.
    #[cfg(feature = "rand")]
    pub fn execute(&mut self) {
        let mut rng = rand::rng();
        self.execute_with_rand(&mut rng);
    }

    /// Runs the actions in a reproducible order drawn from a [`SplitMix64`]
    /// seeded with `seed`. Available without the `rand` feature.
    pub fn execute_seeded(&mut self, seed: u64) {
        let mut rng = SplitMix64::new(seed);
        self.execute_with_rand(&mut rng);
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
    use std::{cell::RefCell, rc::Rc};

    use crate::{
        performance::{ActionAgent, Tester},
        random::FnIndexRng,
    };

    /// A tester with three actions that append their name to a shared log.
    fn logging_tester(loop_count: usize) -> (Tester, Rc<RefCell<Vec<&'static str>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tester = Tester::new(loop_count);
        for name in ["a", "b", "c"] {
            let log = Rc::clone(&log);
            tester.add(ActionAgent::new(
                name,
                Box::new(move || log.borrow_mut().push(name)),
            ));
        }
        (tester, log)
    }

    #[test]
    fn tester_uses_injected_generator() {
        // Always the first remaining action: declaration order every loop.
        let (mut tester, log) = logging_tester(2);
        tester.execute_with_rand(&mut FnIndexRng::new(|_| 0));
        assert_eq!(*log.borrow(), ["a", "b", "c", "a", "b", "c"]);

        // Always the last remaining action, through a trait object.
        let (mut tester, log) = logging_tester(1);
        let mut last = FnIndexRng::new(|upper| upper - 1);
        let dynamic: &mut dyn crate::random::IndexRng = &mut last;
        tester.execute_with_rand(dynamic);
        assert_eq!(*log.borrow(), ["c", "b", "a"]);

        // Out-of-range indices are reduced modulo the remaining count:
        // 4 % 3 = 1 ("b"), then 4 % 2 = 0 ("a"), then "c".
        let (mut tester, log) = logging_tester(1);
        tester.execute_with_rand(&mut FnIndexRng::new(|_| 4));
        assert_eq!(*log.borrow(), ["b", "a", "c"]);
        assert!(tester.actions.iter().all(|agent| agent.count() == 1));
    }

    #[test]
    fn measures_record_display_and_skip_backward_clocks() {
        use std::time::{Duration, SystemTime};

        use crate::performance::{AveragePerformance, PerfomanceMeasure};

        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let forward = PerfomanceMeasure {
            start,
            end: start + Duration::from_millis(5),
        };
        assert_eq!(forward.to_string(), "5ms");
        // An end before the start (the system clock went backwards).
        let backward = PerfomanceMeasure {
            start,
            end: start - Duration::from_millis(1),
        };
        assert_eq!(backward.to_string(), "An Error occur in the time delta");

        let mut average = AveragePerformance::new();
        average.add(&forward);
        average.add(&backward);
        assert_eq!(average.durations, [Duration::from_millis(5)]);
        assert_eq!(average.to_string(), "5ms for 1");

        // A current measure that started "in the future" ends before its
        // start, so it is not recorded.
        average.current = PerfomanceMeasure {
            start: SystemTime::now() + Duration::from_secs(3600),
            end: start,
        };
        average.has_current = true;
        average.end_current();
        assert_eq!(average.durations.len(), 1);
        assert!(!average.has_current);

        average.restart();
        assert!(average.durations.is_empty());
    }

    #[test]
    fn seeded_execution_is_reproducible() {
        let (mut first, first_log) = logging_tester(20);
        first.execute_seeded(42);
        let (mut second, second_log) = logging_tester(20);
        second.execute_seeded(42);
        assert_eq!(*first_log.borrow(), *second_log.borrow());
        assert_eq!(first_log.borrow().len(), 60);

        let (mut other, other_log) = logging_tester(20);
        other.execute_seeded(43);
        assert_ne!(*first_log.borrow(), *other_log.borrow());
    }

    #[cfg(feature = "rand")]
    fn sleep_10ms() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    #[cfg(feature = "rand")]
    fn sleep_100ms() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    #[cfg(feature = "rand")]
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
