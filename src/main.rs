extern crate i3ipc;
use i3ipc::{reply::Workspace, I3Connection};
use log::LevelFilter;
use log::{error, info, trace};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
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

	/// Re-order all existing workspaces, starting at 0. Maintains relative positions
	#[structopt(long)]
	collapse: bool,

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
	workspaces: Vec<Rc<RefCell<Workspace>>>,
	connection: RefCell<I3Connection>,
	monitor_map: HashMap<String, Vec<Rc<RefCell<Workspace>>>>,
}

impl Helper {
	pub fn new() -> Option<Helper> {
		let options = Opt::from_args();

		if options.verbose {
			simple_logging::log_to_stderr(LevelFilter::Trace);
		} else {
			simple_logging::log_to_stderr(LevelFilter::Info);
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

		if direction_count == 0 && !options.monitor && !options.collapse {
			error!("Must select either 'next', 'previous', 'start', or 'end'");
			return None;
		}

		// establish a connection to i3 over a unix socket
		let mut connection = I3Connection::connect().unwrap();

		let workspaces = Self::get_workspaces(&mut connection);

		let monitor_map = Self::get_workspace_map(&workspaces);

		Some(Self {
			options,
			workspaces,
			connection: RefCell::new(connection),
			monitor_map,
		})
	}

	pub fn run(&mut self) {
		if let Some(mut target) = self.get_target_workspace_id() {
			let mut source = self.get_current_workspace().unwrap().borrow().num;
			if target < 0 {
				trace!("Target is < 0. Shifting all workspaces over one");
				self.make_room_for_zero();
				source = self.get_current_workspace().unwrap().borrow().num;
				target = 0;
			}
			trace!("Current workspace id: {}", source);
			trace!("Target workspace id: {}", target);

			if self.options.window {
				self.move_window_to_workspace(target);
				self.focus_workspace(target);
			} else if self.options.workspace {
				if self.options.monitor {
					panic!(
						"You can do this with a simple 'move workspace to output right', so this isn't necessary"
					);
				} else {
					self.swap_workspaces(source, target);
				}
			} else {
				self.focus_workspace(target);
			}
		}

		if self.options.collapse {
			info!("Collapsing...");
			self.do_collapse();
		}
	}

	fn get_workspaces(connection: &mut I3Connection) -> Vec<Rc<RefCell<Workspace>>> {
		let mut workspaces = connection
			.get_workspaces()
			.unwrap()
			.workspaces
			.drain(..)
			.map(|workspace| Rc::new(RefCell::new(workspace)))
			.collect::<Vec<Rc<RefCell<Workspace>>>>();

		// NOTE: is this sorting even necessary?
		workspaces.sort_by(|a, b| a.borrow().num.partial_cmp(&b.borrow().num).unwrap());

		return workspaces;
	}

	// will return a sorted map of workspaces keyed off of monitor
	fn get_workspace_map(
		workspaces: &Vec<Rc<RefCell<Workspace>>>,
	) -> HashMap<String, Vec<Rc<RefCell<Workspace>>>> {
		let mut monitor_map: HashMap<String, Vec<Rc<RefCell<Workspace>>>> = HashMap::new();
		for workspace in workspaces {
			monitor_map
				.entry(workspace.borrow().output.to_string())
				.or_default()
				.push(workspace.clone());
		}
		return monitor_map;
	}

	fn get_target_workspace_id(&self) -> Option<i32> {
		let current_workspace = self.get_current_workspace().unwrap();

		if self.options.end {
			return Some(self.workspaces.last().unwrap().borrow().num + 1);
		} else if self.options.start {
			let first_workspace = self.workspaces.first().unwrap();
			if first_workspace.borrow().focused {
				// we're already the first, so we don't do anything here
				return None;
			}
			return Some(first_workspace.borrow().num - 1);
		}

		if self.options.monitor {
			// TODO: make this less dumb. This just finds the first non-current workspace.
			// Won't work on 3 or more
			for (output, monitor) in self.monitor_map.iter() {
				if output != &current_workspace.borrow().output {
					for workspace in monitor.iter() {
						if workspace.borrow().visible {
							return Some(workspace.borrow().num);
						}
					}
				}
			}
		} else {
			let current_monitor_workspaces = self
				.monitor_map
				.get(&*current_workspace.borrow().output)
				.unwrap();

			if self.options.next {
				for (index, workspace) in current_monitor_workspaces.iter().enumerate() {
					if workspace.borrow().num == current_workspace.borrow().num {
						if index + 1 < current_monitor_workspaces.len() {
							return Some(current_monitor_workspaces[index + 1].borrow().num);
						}
						break;
					}
				}
				// couldn't find a next, so return the first (if just looking)
				return Some(current_monitor_workspaces[0].borrow().num);
			} else if self.options.previous {
				for (index, workspace) in current_monitor_workspaces.iter().enumerate() {
					if workspace.borrow().num == current_workspace.borrow().num {
						if index > 0 {
							return Some(current_monitor_workspaces[index - 1].borrow().num);
						}
						break;
					}
				}
				// couldn't find a previous, so return the last
				return Some(
					current_monitor_workspaces[current_monitor_workspaces.len() - 1]
						.borrow()
						.num,
				);
			}
		}

		return None;
	}

	fn get_current_workspace(&self) -> Option<Rc<RefCell<Workspace>>> {
		let workspace = self
			.workspaces
			.iter()
			.find(|workspace| workspace.borrow().focused);
		if let Some(workspace) = workspace {
			return Some(workspace.clone());
		} else {
			return None;
		}
	}

	fn do_collapse(&self) {
		let mut index = 0;
		for (_, workspaces) in self.monitor_map.iter() {
			for workspace in workspaces {
				self.swap_workspaces(workspace.borrow().num, index);
				workspace.borrow_mut().num = index;
				index += 1;
			}
		}
	}

	fn make_room_for_zero(&self) {
		for monitor_workspaces in self.monitor_map.values() {
			if monitor_workspaces
				.iter()
				.find(|workspace| workspace.borrow().num == 0)
				.is_none()
			{
				continue;
			}

			let mut next = monitor_workspaces.last().unwrap().borrow().num + 1;
			for workspace in monitor_workspaces.iter().rev() {
				// TODO: why doesn't this work?
				// self.move_workspace(workspace.num, next);

				self.connection
					.borrow_mut()
					.run_command(&format!(
						"rename workspace \"{}\" to \"{}\"",
						workspace.borrow().num,
						next
					))
					.unwrap();

				let temp = next;
				next = workspace.borrow().num;
				workspace.borrow_mut().num = temp;
			}
		}
	}

	fn swap_workspaces(&self, source: i32, target: i32) {
		let unique_workspace_id = self.get_unique_workspace_id();
		self.move_workspace(target, unique_workspace_id);
		self.move_workspace(source, target);
		self.move_workspace(unique_workspace_id, source);
	}

	fn get_unique_workspace_id(&self) -> i32 {
		// already sorted. Added an arbitary margin to trying and avoid race conditions
		return self.workspaces.last().unwrap().borrow().num + 10;
	}

	fn move_workspace(&self, source: i32, target: i32) {
		self.run_command(&format!(
			"rename workspace \"{}\" to \"{}\"",
			source, target
		));
	}

	fn focus_workspace(&self, id: i32) {
		self.run_command(&format!("workspace {}", id));
	}

	fn move_window_to_workspace(&self, id: i32) {
		self.run_command(&format!("move container to workspace \"{}\"", id));
	}

	fn run_command(&self, command: &str) {
		self.connection.borrow_mut().run_command(command).unwrap();
	}
}
