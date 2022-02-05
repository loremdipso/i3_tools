extern crate i3ipc;
use i3ipc::{reply::Workspace, I3Connection};
use log::LevelFilter;
use log::{error, info, warn};
use std::cell::RefCell;
use std::collections::HashMap;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "example", about = "An example of StructOpt usage.")]
struct Opt {
	/// Increase verbosity
	#[structopt(short, long)]
	verbose: bool,

	/// Move between monitors rather than between workspaces
	#[structopt(short, long)]
	monitor: bool,

	/// Move a window instead of just the focus
	#[structopt(long)]
	window: bool,

	/// Move the active workspace
	#[structopt(long)]
	workspace: bool,

	/// Append to the end
	#[structopt(long)]
	end: bool,

	/// Prepend to the start
	#[structopt(long)]
	start: bool,

	/// Go to the next
	#[structopt(short, long)]
	next: bool,

	/// Go to the previous
	#[structopt(short, long)]
	previous: bool,
}

fn main() {
	if let Some(mut helper) = Helper::new() {
		helper.run();
	}
}

struct Helper {
	options: Opt,
	workspaces: Vec<SimpleWorkspace>,
	connection: RefCell<I3Connection>,
	map: HashMap<String, Vec<SimpleWorkspace>>,
}

impl Helper {
	pub fn new() -> Option<Helper> {
		let options = Opt::from_args();

		if options.verbose {
			simple_logging::log_to_stderr(LevelFilter::Info);
		} else {
			simple_logging::log_to_stderr(LevelFilter::Error);
		}

		let mut direction_count = 0;
		if options.next {
			direction_count += 1;
		}
		if options.previous {
			direction_count += 1;
		}
		if options.end {
			direction_count += 1;
		}
		if options.start {
			direction_count += 1;
		}

		if direction_count > 1 {
			error!("Can't combine 'next', 'previous', 'start', or 'end' in the same command");
			return None;
		}

		if direction_count == 0 && !options.monitor {
			error!("Must select either 'next', 'previous', 'start', or 'end'");
			return None;
		}

		// establish a connection to i3 over a unix socket
		let mut connection = I3Connection::connect().unwrap();

		let workspaces = Self::get_workspaces(&mut connection);

		let map = Self::get_workspace_map(&workspaces);

		Some(Self {
			options,
			workspaces,
			connection: RefCell::new(connection),
			map,
		})
	}

	fn get_workspaces(connection: &mut I3Connection) -> Vec<SimpleWorkspace> {
		let mut workspaces = connection
			.get_workspaces()
			.unwrap()
			.workspaces
			.iter()
			.map(|workspace| SimpleWorkspace::new(workspace))
			.collect::<Vec<SimpleWorkspace>>();

		// NOTE: is this sorting even necessary?
		workspaces.sort_by(|a, b| a.num.partial_cmp(&b.num).unwrap());

		return workspaces;
	}

	pub fn run(&mut self) {
		if let Some(mut target) = self.get_target_workspace_id() {
			let mut source = self.get_current_workspace().unwrap().num;
			if target < 0 {
				info!("Target is < 0. Shifting all workspaces over one");
				self.make_room_for_zero();
				// reset workspaces
				self.workspaces = Self::get_workspaces(&mut self.connection.borrow_mut());
				self.map = Self::get_workspace_map(&self.workspaces);
				source = self.get_current_workspace().unwrap().num;
				target = 0;
			}
			info!("Current workspace id: {}", source);
			info!("Target workspace id: {}", target);

			if self.options.window {
				self.move_window_to_workspace(target);
				self.focus_workspace(target);
			} else if self.options.workspace {
				if self.options.monitor {
					todo!();
				// self.move_window_to_output(target);
				} else {
					self.swap_workspaces(source, target);
				}
			} else {
				self.focus_workspace(target);
			}
		} else {
			info!("Nothing to do");
		}
	}

	fn run_command(&self, command: &str) {
		self.connection.borrow_mut().run_command(command).unwrap();
	}

	fn make_room_for_zero(&mut self) {
		for monitor_workspaces in self.map.values_mut() {
			if monitor_workspaces
				.iter()
				.find(|workspace| workspace.num == 0)
				.is_none()
			{
				continue;
			}

			let mut next = monitor_workspaces.last().unwrap().num + 1;
			for workspace in monitor_workspaces.iter_mut().rev() {
				// TODO: why doesn't this work?
				// self.move_workspace(workspace.num, next);

				self.connection
					.borrow_mut()
					.run_command(&format!(
						"rename workspace \"{}\" to \"{}\"",
						workspace.num, next
					))
					.unwrap();

				let temp = next;
				next = workspace.num;
				workspace.num = temp;
			}
		}
	}

	fn focus_workspace(&self, id: i32) {
		self.run_command(&format!("workspace {}", id));
	}

	fn move_window_to_workspace(&self, id: i32) {
		self.run_command(&format!("move container to workspace \"{}\"", id));
	}

	fn move_window_to_output(&self, target: i32) {
		todo!();
		// let current_workspace = self.get_current_workspace().unwrap();

		// for output in self.map.keys() {
		// 	if output != &current_workspace.output {
		// 		self.run_command(&format!("move window to output \"{}\"", output));
		// 	}
		// }
	}

	fn swap_workspaces(&self, source: i32, target: i32) {
		let unique_workspace_id = self.get_unique_workspace_id();
		dbg!(unique_workspace_id);
		self.move_workspace(target, unique_workspace_id);
		self.move_workspace(source, target);
		self.move_workspace(unique_workspace_id, source);
	}

	fn get_unique_workspace_id(&self) -> i32 {
		// already sorted. Added an arbitary margin to trying and avoid race conditions
		return self.workspaces.last().unwrap().num + 10;
	}

	fn move_workspace(&self, source: i32, target: i32) {
		self.run_command(&format!(
			"rename workspace \"{}\" to \"{}\"",
			source, target
		));
	}

	fn get_target_workspace_id(&self) -> Option<i32> {
		let current_workspace = self.get_current_workspace().unwrap();

		if self.options.end {
			return Some(self.workspaces.last().unwrap().num + 1);
		} else if self.options.start {
			let first_workspace = self.workspaces.first().unwrap();
			if first_workspace.focused {
				// we're already the first, so we don't do anything here
				return None;
			}
			return Some(first_workspace.num - 1);
		}

		if self.options.monitor {
			// TODO: make this less dumb. This just finds the first non-current workspace.
			// Won't work on 3 or more
			for (output, monitor) in self.map.iter() {
				if output != &current_workspace.output {
					for workspace in monitor.iter() {
						if workspace.visible {
							return Some(workspace.num);
						}
					}
				}
			}
		} else {
			let current_monitor_workspaces = self.map.get(&*current_workspace.output).unwrap();

			if self.options.next {
				for (index, workspace) in current_monitor_workspaces.iter().enumerate() {
					if workspace.num == current_workspace.num {
						if index + 1 < current_monitor_workspaces.len() {
							return Some(current_monitor_workspaces[index + 1].num);
						}
						break;
					}
				}
			// couldn't find a next, so return the first
			// return Some(current_monitor_workspaces[0].num);
			} else if self.options.previous {
				for (index, workspace) in current_monitor_workspaces.iter().enumerate() {
					if workspace.num == current_workspace.num {
						if index > 0 {
							return Some(current_monitor_workspaces[index - 1].num);
						}
						break;
					}
				}
				// couldn't find a previous, so return the last
				// return Some(current_monitor_workspaces[current_monitor_workspaces.len() - 1].num);
			}
		}

		return None;
	}

	fn get_current_workspace(&self) -> Option<SimpleWorkspace> {
		let workspace = self.workspaces.iter().find(|workspace| workspace.focused);
		if let Some(workspace) = workspace {
			return Some(workspace.clone());
		} else {
			return None;
		}
	}

	// will return a sorted map of workspaces keyed off of monitor
	fn get_workspace_map(
		workspaces: &Vec<SimpleWorkspace>,
	) -> HashMap<String, Vec<SimpleWorkspace>> {
		let mut map: HashMap<String, Vec<SimpleWorkspace>> = HashMap::new();
		for workspace in workspaces {
			map.entry(workspace.output.to_string())
				.or_default()
				.push(workspace.clone());
		}
		return map;
	}
}

#[derive(Debug, Clone)]
struct SimpleWorkspace {
	num: i32,
	name: String,
	output: String,
	visible: bool,
	focused: bool,
}

impl SimpleWorkspace {
	pub fn new(workspace: &Workspace) -> Self {
		Self {
			num: workspace.num,
			name: workspace.name.clone(),
			output: workspace.output.clone(),
			visible: workspace.visible,
			focused: workspace.focused,
		}
	}
}
