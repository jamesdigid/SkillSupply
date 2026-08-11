use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::error::{Result, SkillSupportError};
use crate::manifest::models::CapabilityManifest;
use crate::models::DependencyEdge;
use crate::resolver::traits::{DependencyGraph, DependencyResolver};

#[derive(Debug, Clone)]
pub struct PetgraphDependencyGraph {
    graph: DiGraph<String, String>,
    unresolved: Vec<String>,
}

impl PetgraphDependencyGraph {
    fn new(graph: DiGraph<String, String>, unresolved: Vec<String>) -> Self {
        Self { graph, unresolved }
    }
}

impl DependencyGraph for PetgraphDependencyGraph {
    fn nodes(&self) -> Vec<String> {
        self.graph
            .node_indices()
            .filter_map(|index| self.graph.node_weight(index).cloned())
            .collect()
    }

    fn edges(&self) -> Vec<DependencyEdge> {
        self.graph
            .edge_references()
            .filter_map(|edge| {
                let provider = self.graph.node_weight(edge.source())?.clone();
                let dependent = self.graph.node_weight(edge.target())?.clone();
                Some(DependencyEdge {
                    provider,
                    dependent,
                    contract: edge.weight().clone(),
                })
            })
            .collect()
    }

    fn topological_order(&self) -> Result<Vec<String>> {
        let indices = toposort(&self.graph, None).map_err(|cycle| {
            let node = self
                .graph
                .node_weight(cycle.node_id())
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string());
            SkillSupportError::Resolution(format!("dependency cycle detected at {node}"))
        })?;

        Ok(indices
            .into_iter()
            .filter_map(|index| self.graph.node_weight(index).cloned())
            .collect())
    }

    fn unresolved_requirements(&self) -> Vec<String> {
        self.unresolved.clone()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PetgraphDependencyResolver;

impl DependencyResolver for PetgraphDependencyResolver {
    fn build(&self, manifests: &[CapabilityManifest]) -> Result<Box<dyn DependencyGraph>> {
        let mut graph: DiGraph<String, String> = DiGraph::new();
        let mut indices: HashMap<String, NodeIndex> = HashMap::new();
        let mut capability_index: HashMap<String, Vec<String>> = HashMap::new();

        for manifest in manifests {
            let index = graph.add_node(manifest.name.clone());
            indices.insert(manifest.name.clone(), index);

            for capability in &manifest.capabilities {
                capability_index
                    .entry(capability.clone())
                    .or_default()
                    .push(manifest.name.clone());
            }
        }

        let mut unresolved = Vec::new();
        for manifest in manifests {
            let dependent = match indices.get(&manifest.name) {
                Some(index) => *index,
                None => continue,
            };

            for dependency in &manifest.dependencies {
                match capability_index.get(dependency) {
                    Some(providers) => {
                        for provider_name in providers {
                            if let Some(provider) = indices.get(provider_name) {
                                graph.add_edge(*provider, dependent, dependency.clone());
                            }
                        }
                    }
                    None => unresolved.push(dependency.clone()),
                }
            }
        }

        Ok(Box::new(PetgraphDependencyGraph::new(graph, unresolved)))
    }
}
