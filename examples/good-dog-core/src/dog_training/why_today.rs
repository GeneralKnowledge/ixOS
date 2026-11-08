use crate::coaching::{PlanItemRole, SkillState, WhyTodayInput};

/// The plain-English reason shown for choosing an exercise today.
pub fn explain_plan_item(input: &WhyTodayInput<'_>) -> String {
    let dog = input.subject_name;

    if input.preferred_easier {
        return format!("Let’s make this easier today and build up gradually with {dog}.");
    }
    if input.preferred_harder {
        return "You’ve had some good practice with this skill, so we’re trying a tiny step forward."
            .to_string();
    }
    if input.is_starter && input.role == PlanItemRole::Engage {
        return format!("A gentle warm-up to help {dog} tune in.");
    }
    if input.role == PlanItemRole::Everyday {
        return format!("An easy activity that fits naturally into home life with {dog}.");
    }
    match input.skill_state {
        Some(SkillState::BecomingConsistent) => {
            return format!("We’re keeping this one steady so {dog} can build confidence.");
        }
        Some(SkillState::Practising) | Some(SkillState::Introduced) => {
            return format!("Useful practice for a skill you’re working on with {dog}.");
        }
        _ => {}
    }
    if input.role == PlanItemRole::Engage {
        return "A short, friendly start to today’s session.".to_string();
    }
    format!("A practical next step for {dog} today.")
}
