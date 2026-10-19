//contents of graph.rs

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub dependencies: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub struct TaskGraph {
    tasks: BTreeMap<String, Task>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("duplicate task '{0}'")]
    Duplicate(String),

    #[error("task '{task}' depends on unknown task '{dependency}'")]
    UnknownDependency { task: String, dependency: String },

    #[error("dependency cycle detected: {0:?}")]
    Cycle(Vec<String>),
}

impl TaskGraph {
    pub fn add_task<I, S>(
        &mut self,
        id: impl Into<String>,
        dependencies: I,
    ) -> Result<(), GraphError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let id = id.into();

        if self.tasks.contains_key(&id) {
            return Err(GraphError::Duplicate(id));
        }

        let dependencies = dependencies.into_iter().map(Into::into).collect();

        self.tasks.insert(id.clone(), Task { id, dependencies });

        Ok(())
    }

    /// Produces a deterministic dependency-first build order.
    pub fn execution_order(&self) -> Result<Vec<String>, GraphError> {
        for task in self.tasks.values() {
            for dependency in &task.dependencies {
                if !self.tasks.contains_key(dependency) {
                    return Err(GraphError::UnknownDependency {
                        task: task.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }

        let mut indegree: BTreeMap<String, usize> = self
            .tasks
            .iter()
            .map(|(id, task)| (id.clone(), task.dependencies.len()))
            .collect();

        let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for task in self.tasks.values() {
            for dependency in &task.dependencies {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .insert(task.id.clone());
            }
        }

        let mut ready: VecDeque<String> = indegree
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut result = Vec::with_capacity(self.tasks.len());

        while let Some(id) = ready.pop_front() {
            result.push(id.clone());

            if let Some(children) = dependents.get(&id) {
                for child in children {
                    let count = indegree.get_mut(child).expect("registered dependent");

                    *count -= 1;

                    if *count == 0 {
                        ready.push_back(child.clone());
                    }
                }
            }
        }

        if result.len() != self.tasks.len() {
            let cycle = indegree
                .into_iter()
                .filter(|(_, count)| *count > 0)
                .map(|(id, _)| id)
                .collect();

            return Err(GraphError::Cycle(cycle));
        }

        Ok(result)
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}
