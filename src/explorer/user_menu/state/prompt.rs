use crate::explorer::user_menu::parse::{self, Prompt};
use crate::text_field::TextField;

/// `Mode::UserMenuPrompt`: collecting answers to a `Commands` item's
/// own `!?Label?Default!` placeholders (`parse::extract_prompts`)
/// before running it -- one text field shown at a time, in
/// first-appearance order, same as real Far Manager's own "each
/// distinct label prompts once" behavior (`parse::extract_prompts`'s
/// own doc comment).
pub struct UserMenuPromptState {
    /// The item's own commands, with `!&` already substituted --
    /// `!?Label?Default!` placeholders are still present until
    /// `accept_current` finishes collecting every answer.
    commands: Vec<String>,
    prompts: Vec<Prompt>,
    current: usize,
    answers: Vec<(String, String)>,
    pub value: TextField,
}

impl UserMenuPromptState {
    /// `prompts` must be non-empty -- callers only build this once
    /// `parse::extract_prompts` on `commands` actually found at least
    /// one placeholder; a `Commands` item with none just runs directly
    /// instead (`explorer::user_menu::input`).
    pub fn new(commands: Vec<String>, prompts: Vec<Prompt>) -> Self {
        assert!(!prompts.is_empty(), "UserMenuPromptState needs at least one prompt to collect");
        let value = TextField::with_text(prompts[0].default.clone());
        Self { commands, prompts, current: 0, answers: Vec::new(), value }
    }

    /// The label to show above the input field for whichever prompt is
    /// currently being asked.
    pub fn current_label(&self) -> &str {
        &self.prompts[self.current].label
    }

    /// How many prompts remain including this one, and the total --
    /// e.g. "2 of 3", for the popup's own title/footer.
    pub fn progress(&self) -> (usize, usize) {
        (self.current + 1, self.prompts.len())
    }

    /// Accepts the current field's value as this prompt's answer.
    /// Advances to the next prompt (pre-filling its own default) and
    /// returns `None` if there is one; once every prompt has an answer,
    /// substitutes them all into `commands` and returns the finished,
    /// ready-to-run list instead.
    pub fn accept_current(&mut self) -> Option<Vec<String>> {
        self.answers.push((self.prompts[self.current].label.clone(), self.value.text().to_string()));
        self.current += 1;

        if self.current < self.prompts.len() {
            self.value.set_text(self.prompts[self.current].default.clone());
            None
        } else {
            Some(self.commands.iter().map(|command| parse::substitute_prompts(command, &self.answers)).collect())
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_prefilled_with_the_first_prompts_default() {
        let prompts = vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }];
        let state = UserMenuPromptState::new(vec!["git checkout !?Branch?Master!".to_string()], prompts);
        assert_eq!(state.value.text(), "Master");
        assert_eq!(state.current_label(), "Branch");
        assert_eq!(state.progress(), (1, 1));
    }

    #[test]
    fn accept_current_advances_to_the_next_prompt() {
        let prompts = vec![
            Prompt { label: "First".to_string(), default: "a".to_string() },
            Prompt { label: "Second".to_string(), default: "b".to_string() },
        ];
        let mut state = UserMenuPromptState::new(vec!["echo !?First?a! !?Second?b!".to_string()], prompts);

        let result = state.accept_current();

        assert_eq!(result, None, "should not finish yet -- one prompt left");
        assert_eq!(state.current_label(), "Second");
        assert_eq!(state.value.text(), "b", "pre-filled with the next prompt's own default");
    }

    #[test]
    fn accept_current_returns_the_substituted_commands_once_every_prompt_is_answered() {
        let prompts = vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }];
        let mut state = UserMenuPromptState::new(vec!["git checkout !?Branch?Master!".to_string()], prompts);
        state.value.set_text("feature/x");

        let result = state.accept_current();

        assert_eq!(result, Some(vec!["git checkout feature/x".to_string()]));
    }
}
