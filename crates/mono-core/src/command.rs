#[derive(Debug)]
pub struct Step {
    pub program: String,
    pub args: Vec<String>,
    pub detached: bool,
}
#[derive(Debug, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub confirm: bool,
    pub format_device: Option<String>,
    pub notes: Vec<String>,
    pub description: Option<String>,
    pub delete_args: Option<Vec<String>>,
}
impl Plan {
    pub fn add(&mut self, program: &str, args: &[&str]) {
        self.steps.push(Step {
            program: format!("/usr/bin/{program}"),
            args: args.iter().map(|s| s.to_string()).collect(),
            detached: false,
        });
    }
    pub fn describe(&self) {
        if let Some(description) = &self.description {
            println!("{description}");
            return;
        }
        for note in &self.notes {
            println!("{note}");
        }
        for step in &self.steps {
            println!("{} {:?}", step.program, step.args);
        }
    }
}
