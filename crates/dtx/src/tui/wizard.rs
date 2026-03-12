//! Service add/edit wizard for TUI.

use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub enum WizardStep {
    Name,
    Command,
    Port,
    Deps,
    Confirm,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WizardState {
    pub step: WizardStep,
    pub is_edit: bool,
    pub original_name: Option<String>,
    pub name: String,
    pub command: String,
    pub port: String,
    pub deps: Vec<String>,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub available_services: Vec<String>,
    pub selected_dep_index: usize,
    pub dep_selected: HashSet<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WizardResult {
    pub name: String,
    pub command: String,
    pub port: Option<u16>,
    pub deps: Vec<String>,
}

pub enum WizardAction {
    Continue,
    Cancel,
    Complete(WizardResult),
}

impl WizardState {
    pub fn new_add(available_services: Vec<String>) -> Self {
        Self {
            step: WizardStep::Name,
            is_edit: false,
            original_name: None,
            name: String::new(),
            command: String::new(),
            port: String::new(),
            deps: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            available_services,
            selected_dep_index: 0,
            dep_selected: HashSet::new(),
            error: None,
        }
    }

    pub fn new_edit(
        name: &str,
        command: &str,
        port: Option<u16>,
        deps: Vec<String>,
        available_services: Vec<String>,
    ) -> Self {
        let dep_selected: HashSet<String> = deps.iter().cloned().collect();
        Self {
            step: WizardStep::Command,
            is_edit: true,
            original_name: Some(name.to_string()),
            name: name.to_string(),
            command: command.to_string(),
            port: port.map(|p| p.to_string()).unwrap_or_default(),
            deps,
            input_buffer: command.to_string(),
            input_cursor: command.len(),
            available_services,
            selected_dep_index: 0,
            dep_selected,
            error: None,
        }
    }

    fn sync_input_to_field(&mut self) {
        match self.step {
            WizardStep::Name => self.name = self.input_buffer.clone(),
            WizardStep::Command => self.command = self.input_buffer.clone(),
            WizardStep::Port => self.port = self.input_buffer.clone(),
            WizardStep::Deps | WizardStep::Confirm => {}
        }
    }

    fn load_field_to_input(&mut self) {
        match self.step {
            WizardStep::Name => {
                self.input_buffer = self.name.clone();
                self.input_cursor = self.input_buffer.len();
            }
            WizardStep::Command => {
                self.input_buffer = self.command.clone();
                self.input_cursor = self.input_buffer.len();
            }
            WizardStep::Port => {
                self.input_buffer = self.port.clone();
                self.input_cursor = self.input_buffer.len();
            }
            WizardStep::Deps => {
                self.selected_dep_index = 0;
            }
            WizardStep::Confirm => {}
        }
    }

    fn validate_step(&mut self) -> bool {
        self.error = None;
        match self.step {
            WizardStep::Name => {
                let name = self.name.trim();
                if name.is_empty() {
                    self.error = Some("Name cannot be empty".to_string());
                    return false;
                }
                if name.len() < 2 || name.len() > 63 {
                    self.error = Some("Name must be 2-63 characters".to_string());
                    return false;
                }
                if !name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    self.error =
                        Some("Name: lowercase letters, digits, hyphens only".to_string());
                    return false;
                }
                true
            }
            WizardStep::Command => {
                if self.command.trim().is_empty() {
                    self.error = Some("Command cannot be empty".to_string());
                    return false;
                }
                true
            }
            WizardStep::Port => {
                if self.port.is_empty() {
                    return true; // port is optional
                }
                match self.port.parse::<u16>() {
                    Ok(p) if p >= 1024 => true,
                    Ok(_) => {
                        self.error = Some("Port must be >= 1024".to_string());
                        false
                    }
                    Err(_) => {
                        self.error = Some("Invalid port number".to_string());
                        false
                    }
                }
            }
            WizardStep::Deps | WizardStep::Confirm => true,
        }
    }

    fn next_step(&mut self) -> WizardAction {
        self.sync_input_to_field();
        if !self.validate_step() {
            return WizardAction::Continue;
        }

        self.step = match self.step {
            WizardStep::Name => WizardStep::Command,
            WizardStep::Command => WizardStep::Port,
            WizardStep::Port => {
                self.deps = self.dep_selected.iter().cloned().collect();
                if self.available_services.is_empty() {
                    WizardStep::Confirm
                } else {
                    WizardStep::Deps
                }
            }
            WizardStep::Deps => {
                self.deps = self.dep_selected.iter().cloned().collect();
                WizardStep::Confirm
            }
            WizardStep::Confirm => return self.complete(),
        };
        self.load_field_to_input();
        WizardAction::Continue
    }

    fn prev_step(&mut self) -> WizardAction {
        self.sync_input_to_field();
        self.error = None;
        self.step = match self.step {
            WizardStep::Name => return WizardAction::Cancel,
            WizardStep::Command => {
                if self.is_edit {
                    return WizardAction::Cancel;
                }
                WizardStep::Name
            }
            WizardStep::Port => WizardStep::Command,
            WizardStep::Deps => WizardStep::Port,
            WizardStep::Confirm => {
                if self.available_services.is_empty() {
                    WizardStep::Port
                } else {
                    WizardStep::Deps
                }
            }
        };
        self.load_field_to_input();
        WizardAction::Continue
    }

    fn complete(&self) -> WizardAction {
        let port = if self.port.is_empty() {
            None
        } else {
            self.port.parse::<u16>().ok()
        };

        WizardAction::Complete(WizardResult {
            name: self.name.clone(),
            command: self.command.clone(),
            port,
            deps: self.deps.clone(),
        })
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyCode) -> WizardAction {
        use crossterm::event::KeyCode;

        match self.step {
            WizardStep::Deps => self.handle_key_deps(key),
            WizardStep::Confirm => match key {
                KeyCode::Enter => self.complete(),
                KeyCode::Esc => self.prev_step(),
                _ => WizardAction::Continue,
            },
            _ => match key {
                KeyCode::Enter => self.next_step(),
                KeyCode::Esc => self.prev_step(),
                KeyCode::Backspace => {
                    if self.input_cursor > 0 {
                        self.input_buffer.remove(self.input_cursor - 1);
                        self.input_cursor -= 1;
                    }
                    self.error = None;
                    WizardAction::Continue
                }
                KeyCode::Char(c) => {
                    self.input_buffer.insert(self.input_cursor, c);
                    self.input_cursor += 1;
                    self.error = None;
                    WizardAction::Continue
                }
                KeyCode::Left => {
                    self.input_cursor = self.input_cursor.saturating_sub(1);
                    WizardAction::Continue
                }
                KeyCode::Right => {
                    if self.input_cursor < self.input_buffer.len() {
                        self.input_cursor += 1;
                    }
                    WizardAction::Continue
                }
                KeyCode::Tab => self.next_step(),
                _ => WizardAction::Continue,
            },
        }
    }

    fn handle_key_deps(&mut self, key: crossterm::event::KeyCode) -> WizardAction {
        use crossterm::event::KeyCode;
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_dep_index > 0 {
                    self.selected_dep_index -= 1;
                }
                WizardAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_dep_index < self.available_services.len().saturating_sub(1) {
                    self.selected_dep_index += 1;
                }
                WizardAction::Continue
            }
            KeyCode::Char(' ') => {
                if let Some(svc) = self.available_services.get(self.selected_dep_index) {
                    let svc = svc.clone();
                    if self.dep_selected.contains(&svc) {
                        self.dep_selected.remove(&svc);
                    } else {
                        self.dep_selected.insert(svc);
                    }
                }
                WizardAction::Continue
            }
            KeyCode::Enter | KeyCode::Tab => self.next_step(),
            KeyCode::Esc => self.prev_step(),
            _ => WizardAction::Continue,
        }
    }

    pub fn step_label(&self) -> &str {
        match self.step {
            WizardStep::Name => "Name",
            WizardStep::Command => "Command",
            WizardStep::Port => "Port (optional)",
            WizardStep::Deps => "Dependencies",
            WizardStep::Confirm => "Confirm",
        }
    }

    pub fn step_number(&self) -> (usize, usize) {
        let current = match self.step {
            WizardStep::Name => 1,
            WizardStep::Command => 2,
            WizardStep::Port => 3,
            WizardStep::Deps => 4,
            WizardStep::Confirm => {
                if self.available_services.is_empty() {
                    4
                } else {
                    5
                }
            }
        };
        let total = if self.available_services.is_empty() {
            4
        } else {
            5
        };
        (current, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    #[test]
    fn test_wizard_new_add() {
        let state = WizardState::new_add(vec!["api".to_string()]);
        assert_eq!(state.step, WizardStep::Name);
        assert!(!state.is_edit);
    }

    #[test]
    fn test_wizard_name_validation() {
        let mut state = WizardState::new_add(vec![]);
        // Empty name fails
        state.input_buffer = String::new();
        assert!(matches!(
            state.handle_key(KeyCode::Enter),
            WizardAction::Continue
        ));
        assert!(state.error.is_some());

        // Valid name advances
        state.input_buffer = "api".to_string();
        state.input_cursor = 3;
        state.error = None;
        assert!(matches!(
            state.handle_key(KeyCode::Enter),
            WizardAction::Continue
        ));
        assert_eq!(state.step, WizardStep::Command);
    }

    #[test]
    fn test_wizard_complete_flow() {
        let mut state = WizardState::new_add(vec![]);
        // Name
        state.input_buffer = "api".to_string();
        state.input_cursor = 3;
        state.handle_key(KeyCode::Enter);
        // Command
        state.input_buffer = "npm start".to_string();
        state.input_cursor = 9;
        state.handle_key(KeyCode::Enter);
        // Port
        state.input_buffer = "3000".to_string();
        state.input_cursor = 4;
        state.handle_key(KeyCode::Enter);
        // No deps available, goes to Confirm
        assert_eq!(state.step, WizardStep::Confirm);
        let result = state.handle_key(KeyCode::Enter);
        assert!(matches!(result, WizardAction::Complete(_)));
        if let WizardAction::Complete(r) = result {
            assert_eq!(r.name, "api");
            assert_eq!(r.command, "npm start");
            assert_eq!(r.port, Some(3000));
        }
    }

    #[test]
    fn test_wizard_port_validation() {
        let mut state = WizardState::new_add(vec![]);
        state.step = WizardStep::Port;
        state.input_buffer = "80".to_string();
        state.input_cursor = 2;
        state.handle_key(KeyCode::Enter);
        assert!(state.error.is_some());
        assert_eq!(state.step, WizardStep::Port);

        state.input_buffer = "3000".to_string();
        state.input_cursor = 4;
        state.error = None;
        state.handle_key(KeyCode::Enter);
        assert!(state.error.is_none());
    }

    #[test]
    fn test_wizard_cancel() {
        let mut state = WizardState::new_add(vec![]);
        let action = state.handle_key(KeyCode::Esc);
        assert!(matches!(action, WizardAction::Cancel));
    }
}
