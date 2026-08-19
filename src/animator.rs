//! The animations one page currently has running.
//!
//! Styles are recomputed from the stylesheet whenever anything changes, so an
//! animated value cannot live in the cascade — the next recompute would throw
//! it away. It lives here instead, and is folded into the freshly cascaded
//! values on the way past: the cascade decides where a property is heading, and
//! this decides where it has got to.

use rustc_hash::FxHashMap;

use crate::css::animation::{apply_animated, interpolate_value};
use crate::css::{ComputedValues, Easing, KeyframesRule, LengthContext};

/// A transition part-way from one value to another.
#[derive(Debug, Clone, PartialEq)]
struct Running {
    from: String,
    to: String,
    /// Page time, in seconds, at which the change was noticed.
    started: f32,
    duration: f32,
    delay: f32,
    easing: Easing,
}

impl Running {
    /// Where this transition has got to at page time `now`, or `None` once it
    /// has arrived.
    fn value_at(&self, now: f32) -> Option<String> {
        let active = now - self.started - self.delay;
        if active < 0.0 {
            return Some(self.from.clone());
        }
        if active >= self.duration {
            return None;
        }
        let progress = self.easing.apply(active / self.duration);
        Some(interpolate_value(&self.from, &self.to, progress))
    }
}

/// The clock and the state behind one page's animations.
pub struct Animator {
    started: std::time::Instant,
    /// The value each transitioned property last cascaded to, so a change can
    /// be noticed at all.
    previous: FxHashMap<(u32, String), String>,
    running: FxHashMap<(u32, String), Running>,
    /// Whether the last pass left anything still moving. Read by the frame loop
    /// to decide whether the page needs laying out again.
    active: bool,
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}

impl Animator {
    pub fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
            previous: FxHashMap::default(),
            running: FxHashMap::default(),
            active: false,
        }
    }

    /// Seconds since the page was built.
    pub fn now(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// Whether anything was still moving after the last [`Self::apply`].
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Fold the running transitions and `@keyframes` animations into a freshly
    /// cascaded set of styles.
    ///
    /// Transitions come first and animations over the top of them, which is the
    /// order CSS resolves the two in when both touch a property.
    pub fn apply(
        &mut self,
        styles: &mut FxHashMap<u32, ComputedValues>,
        keyframes: &[KeyframesRule],
        ctx: LengthContext,
    ) {
        let now = self.now();
        let mut active = false;

        let ids: Vec<u32> = styles.keys().copied().collect();
        for id in ids {
            let Some(computed) = styles.get(&id) else {
                continue;
            };
            let transitions = computed.transitions.clone();
            let animation = computed.animation.clone();

            // Transitions: notice what the cascade has changed, then show how
            // far along the change is rather than the change itself.
            if !transitions.entries.is_empty() {
                for property in transitionable_properties(&transitions) {
                    let Some(target) = computed.animatable_value(&property) else {
                        continue;
                    };
                    let key = (id, property.clone());
                    let previous = self.previous.insert(key.clone(), target.clone());

                    match previous {
                        Some(before) if before != target => {
                            let Some(entry) = transitions.covering(&property) else {
                                self.running.remove(&key);
                                continue;
                            };
                            // Reversing part-way starts from where the element
                            // actually is, not from where it set out.
                            let from = self
                                .running
                                .get(&key)
                                .and_then(|run| run.value_at(now))
                                .unwrap_or(before);
                            self.running.insert(
                                key.clone(),
                                Running {
                                    from,
                                    to: target,
                                    started: now,
                                    duration: entry.duration,
                                    delay: entry.delay,
                                    easing: entry.easing,
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }

            let mut updated = styles.get(&id).cloned();
            for (key, run) in self.running.iter() {
                if key.0 != id {
                    continue;
                }
                let Some(value) = run.value_at(now) else {
                    continue;
                };
                active = true;
                if let Some(current) = updated.take() {
                    updated = Some(apply_animated(current, &key.1, &value, ctx));
                }
            }

            // `@keyframes`: the timeline says what the value is outright, so
            // nothing has to be remembered between frames.
            if !animation.is_none()
                && let Some(rule) = keyframes
                    .iter()
                    .find(|rule| rule.name.eq_ignore_ascii_case(&animation.name))
            {
                // An animation that has not started yet, or has finished
                // without being asked to hold, contributes nothing — but an
                // unfinished one keeps the page repainting.
                if animation.iterations.is_none() {
                    active = true;
                }
                if let Some(progress) = animation.progress_at(now) {
                    if progress < 1.0 || animation.iterations.is_none() {
                        active = true;
                    }
                    for property in rule.properties() {
                        let Some(value) = rule.value_at(&property, progress) else {
                            continue;
                        };
                        if let Some(current) = updated.take() {
                            updated = Some(apply_animated(current, &property, &value, ctx));
                        }
                    }
                } else if now < animation.delay + animation.duration {
                    // Still waiting out its delay.
                    active = true;
                }
            }

            if let Some(updated) = updated {
                styles.insert(id, updated);
            }
        }

        // A transition that has arrived is over; leaving it would make every
        // later frame think the page was still moving.
        self.running
            .retain(|_, run| now - run.started - run.delay < run.duration);
        self.active = active || !self.running.is_empty();
    }

    /// Forget everything, as when a page is replaced.
    pub fn reset(&mut self) {
        self.started = std::time::Instant::now();
        self.previous.clear();
        self.running.clear();
        self.active = false;
    }
}

/// The properties a set of transitions might cover.
///
/// A named property is watched on its own; `all` means every property this
/// engine knows how to blend, since any of them could change.
fn transitionable_properties(transitions: &crate::css::Transitions) -> Vec<String> {
    let mut properties = Vec::new();
    for entry in &transitions.entries {
        if entry.property == "all" {
            for name in ANIMATABLE_PROPERTIES {
                if !properties.iter().any(|p| p == name) {
                    properties.push(name.to_string());
                }
            }
        } else if !properties.contains(&entry.property) {
            properties.push(entry.property.clone());
        }
    }
    properties
}

/// The properties `transition: all` watches.
pub const ANIMATABLE_PROPERTIES: &[&str] = &[
    "color",
    "background-color",
    "border-color",
    "border-radius",
    "border-width",
    "width",
    "height",
    "min-width",
    "max-width",
    "top",
    "right",
    "bottom",
    "left",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "font-size",
    "line-height",
    "row-gap",
    "column-gap",
    "flex-grow",
    "flex-shrink",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::parser::parse_stylesheet;

    /// Cascade one element's declarations into computed values.
    fn computed(declarations: &str) -> ComputedValues {
        let mut values = crate::css::initial_values(1.0);
        for decl in crate::css::parse_declarations(declarations) {
            values = values.from_declaration_with_ctx(&decl, LengthContext::default());
        }
        values
    }

    fn styles(declarations: &str) -> FxHashMap<u32, ComputedValues> {
        let mut map = FxHashMap::default();
        map.insert(1u32, computed(declarations));
        map
    }

    #[test]
    fn a_transition_does_nothing_until_the_value_changes() {
        let mut animator = Animator::new();
        let mut map = styles("transition: width 1s linear; width: 100px");
        animator.apply(&mut map, &[], LengthContext::default());
        assert_eq!(map[&1].width, Some(100.0));
        assert!(!animator.is_active());
    }

    #[test]
    fn a_changed_value_starts_from_where_it_was() {
        let mut animator = Animator::new();
        let mut map = styles("transition: width 100s linear; width: 100px");
        animator.apply(&mut map, &[], LengthContext::default());

        let mut moved = styles("transition: width 100s linear; width: 200px");
        animator.apply(&mut moved, &[], LengthContext::default());

        let width = moved[&1].width.expect("width is set");
        assert!(
            (100.0..101.0).contains(&width),
            "a hundred-second transition has barely begun: {width}"
        );
        assert!(animator.is_active());
    }

    #[test]
    fn a_property_with_no_transition_jumps_straight_to_its_new_value() {
        let mut animator = Animator::new();
        let mut map = styles("width: 100px");
        animator.apply(&mut map, &[], LengthContext::default());
        let mut moved = styles("width: 200px");
        animator.apply(&mut moved, &[], LengthContext::default());
        assert_eq!(moved[&1].width, Some(200.0));
        assert!(!animator.is_active());
    }

    #[test]
    fn a_transition_that_has_run_its_course_stops_being_active() {
        let mut animator = Animator::new();
        let mut map = styles("transition: width 0.001s linear; width: 100px");
        animator.apply(&mut map, &[], LengthContext::default());
        let mut moved = styles("transition: width 0.001s linear; width: 200px");
        animator.apply(&mut moved, &[], LengthContext::default());

        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut settled = styles("transition: width 0.001s linear; width: 200px");
        animator.apply(&mut settled, &[], LengthContext::default());
        assert_eq!(settled[&1].width, Some(200.0));
        assert!(!animator.is_active());
    }

    #[test]
    fn transition_all_notices_a_colour_change() {
        let mut animator = Animator::new();
        let mut map = styles("transition: all 100s linear; color: #000000");
        animator.apply(&mut map, &[], LengthContext::default());
        let mut moved = styles("transition: all 100s linear; color: #ffffff");
        animator.apply(&mut moved, &[], LengthContext::default());

        let color = moved[&1].color.expect("colour is set");
        assert!(
            color[0] < 20,
            "barely moved off black after an instant: {color:?}"
        );
        assert!(animator.is_active());
    }

    fn keyframes(source: &str) -> Vec<KeyframesRule> {
        parse_stylesheet(source).keyframes
    }

    #[test]
    fn an_infinite_animation_keeps_the_page_moving() {
        let rules = keyframes("@keyframes grow { from { width: 0px } to { width: 100px } }");
        let mut animator = Animator::new();
        let mut map = styles("animation: grow 100s linear infinite");
        animator.apply(&mut map, &rules, LengthContext::default());

        let width = map[&1].width.expect("the animation sets a width");
        assert!(width < 1.0, "only just started: {width}");
        assert!(animator.is_active());
    }

    #[test]
    fn an_animation_naming_a_rule_that_does_not_exist_changes_nothing() {
        let mut animator = Animator::new();
        let mut map = styles("animation: missing 1s linear; width: 50px");
        animator.apply(&mut map, &[], LengthContext::default());
        assert_eq!(map[&1].width, Some(50.0));
        assert!(!animator.is_active());
    }

    #[test]
    fn a_finished_animation_with_forwards_holds_its_last_frame() {
        let rules = keyframes("@keyframes grow { from { width: 0px } to { width: 100px } }");
        let mut animator = Animator::new();
        // A duration this short is over before the first frame is drawn.
        let mut map = styles("animation: grow 0.001s linear forwards");
        std::thread::sleep(std::time::Duration::from_millis(10));
        animator.apply(&mut map, &rules, LengthContext::default());
        assert_eq!(map[&1].width, Some(100.0));
        assert!(!animator.is_active(), "it has nowhere left to go");
    }

    #[test]
    fn resetting_forgets_what_was_running() {
        let mut animator = Animator::new();
        let mut map = styles("transition: width 100s linear; width: 100px");
        animator.apply(&mut map, &[], LengthContext::default());
        let mut moved = styles("transition: width 100s linear; width: 200px");
        animator.apply(&mut moved, &[], LengthContext::default());
        assert!(animator.is_active());

        animator.reset();
        assert!(!animator.is_active());
        let mut again = styles("transition: width 100s linear; width: 200px");
        animator.apply(&mut again, &[], LengthContext::default());
        assert_eq!(
            again[&1].width,
            Some(200.0),
            "with no history, there is nothing to ease from"
        );
    }
}
