// Copyright (C) 2019-2023 Aleo Systems Inc.
// This file is part of the snarkVM library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod helpers;
pub use helpers::*;

use console::{
    network::prelude::*,
    program::{Identifier, Literal, Plaintext, ProgramID},
};
use ledger_block::{Execution, Input, Output, Transition};

use indexmap::IndexMap;

#[derive(Debug, Clone)]
pub struct Restrictions<N: Network> {
    /// The set of program IDs that are restricted from being executed.
    /// e.g. `restricted.aleo` => `..` (all blocks)
    /// e.g. `restricted.aleo` => `10..` (from block 10 onwards)
    /// e.g. `restricted.aleo` => `..10` (up to block 10)
    /// e.g. `restricted.aleo` => `10..20` (from block 10 to block 20)
    programs: IndexMap<ProgramID<N>, BlockRange>,
    /// The set of `(program ID, function name)` pairs that are restricted from being executed.
    /// e.g. `restricted.aleo/foo` => `..` (all blocks)
    /// e.g. `restricted.aleo/foo` => `10..` (from block 10 onwards)
    /// e.g. `restricted.aleo/foo` => `..10` (up to block 10)
    /// e.g. `restricted.aleo/foo` => `10..20` (from block 10 to block 20)
    functions: IndexMap<(ProgramID<N>, Identifier<N>), BlockRange>,
    /// The set of `(program ID, function name, argument)` triples that are restricted from being executed.
    /// e.g. `restricted.aleo/bar _ aleo1zkpxxxxx _ _` => `..` (all blocks)
    /// e.g. `restricted.aleo/bar _ aleo1zkpxxxxx _ _` => `10..` (from block 10 onwards)
    /// e.g. `restricted.aleo/bar _ aleo1zkpxxxxx _ _` => `..10` (up to block 10)
    /// e.g. `restricted.aleo/bar _ aleo1zkpxxxxx _ _` => `10..20` (from block 10 to block 20)
    ///
    /// Note: This design intentionally minimizes the number of total lookups required to check
    /// for restrictions when a transition matches the described profile. In summary:
    /// - When a transition does not match the program ID or function name, the total lookup cost is `O(1)`.
    /// - When a transition matches the program ID & function name, the initial lookup cost is `O(num_inputs + num_outputs)`.
    ///    - If an input or output index does not match, the additional lookup cost is `0`.
    ///    - If an input or output index matches, the additional lookup cost is `O(n)` for `n` arguments with the same index.
    arguments: IndexMap<(ProgramID<N>, Identifier<N>), IndexMap<(bool, u16), IndexMap<Literal<N>, BlockRange>>>,
}

impl<N: Network> Default for Restrictions<N> {
    /// Initializes a new `Restrictions` instance.
    fn default() -> Self {
        Self::new()
    }
}

impl<N: Network> Restrictions<N> {
    /// Initializes a new `Restrictions` instance.
    pub fn new() -> Self {
        Self { programs: IndexMap::new(), functions: IndexMap::new(), arguments: IndexMap::new() }
    }
}

impl<N: Network> Restrictions<N> {
    /// Returns the set of program IDs that are restricted from being executed.
    pub fn programs(&self) -> &IndexMap<ProgramID<N>, BlockRange> {
        &self.programs
    }

    /// Returns the set of `(program ID, function ID)` pairs that are restricted from being executed.
    pub fn functions(&self) -> &IndexMap<(ProgramID<N>, Identifier<N>), BlockRange> {
        &self.functions
    }

    /// Returns the set of `(program ID, function ID, argument)` triples that are restricted from being executed.
    pub fn arguments(
        &self,
    ) -> &IndexMap<(ProgramID<N>, Identifier<N>), IndexMap<(bool, u16), IndexMap<Literal<N>, BlockRange>>> {
        &self.arguments
    }
}

impl<N: Network> Restrictions<N> {
    /// Returns `true` if the given program ID is restricted from being executed.
    pub fn is_program_restricted(&self, program_id: &ProgramID<N>, block_height: u32) -> bool {
        self.programs.get(program_id).map_or(false, |range| range.contains(block_height))
    }

    /// Returns `true` if the given `(program ID, function name)` pair is restricted from being executed.
    pub fn is_function_restricted(
        &self,
        program_id: &ProgramID<N>,
        function_name: &Identifier<N>,
        block_height: u32,
    ) -> bool {
        self.functions.get(&(*program_id, *function_name)).map_or(false, |range| range.contains(block_height))
    }

    /// Returns `true` if the given `(program ID, function ID, argument)` triple is restricted from being executed.
    pub fn is_argument_restricted(&self, transition: &Transition<N>, block_height: u32) -> bool {
        self.arguments.get(&(*transition.program_id(), *transition.function_name())).map_or(false, |entries| {
            // Check if any argument is restricted and return `true` if one is found.
            for ((is_input, index), arguments) in entries {
                match is_input {
                    true => {
                        if let Some(argument) = transition.inputs().get(*index as usize) {
                            match argument {
                                Input::Constant(_, Some(plaintext)) | Input::Public(_, Some(plaintext)) => {
                                    match plaintext {
                                        Plaintext::Literal(literal, _) => {
                                            if let Some(range) = arguments.get(literal) {
                                                if range.contains(block_height) {
                                                    return true;
                                                }
                                            }
                                        }
                                        Plaintext::Struct(..) | Plaintext::Array(..) => continue,
                                    }
                                }
                                _ => continue,
                            }
                        }
                    }
                    false => {
                        if let Some(argument) = transition.outputs().get(*index as usize) {
                            match argument {
                                Output::Constant(_, Some(plaintext)) | Output::Public(_, Some(plaintext)) => {
                                    match plaintext {
                                        Plaintext::Literal(literal, _) => {
                                            if let Some(range) = arguments.get(literal) {
                                                if range.contains(block_height) {
                                                    return true;
                                                }
                                            }
                                        }
                                        Plaintext::Struct(..) | Plaintext::Array(..) => continue,
                                    }
                                }
                                _ => continue,
                            }
                        }
                    }
                }
            }
            // Otherwise, return `false`.
            false
        })
    }
}

impl<N: Network> Restrictions<N> {
    /// Returns `true` if the given execution contains any restricted transitions for the given block height.
    pub fn contains_restricted_transitions(&self, execution: &Execution<N>, block_height: u32) -> bool {
        // Check if any transition is restricted.
        execution.transitions().any(|transition| {
            // Retrieve the program ID.
            let program_id = transition.program_id();
            // Retrieve the function name.
            let function_name = transition.function_name();

            // If the program is restricted, then the transition is restricted.
            if self.is_program_restricted(program_id, block_height) {
                return true;
            }
            // If the function is restricted, then the transition is restricted.
            if self.is_function_restricted(program_id, function_name, block_height) {
                return true;
            }
            // If any argument is restricted, then the transition is restricted.
            if self.is_argument_restricted(transition, block_height) {
                return true;
            }
            // Otherwise, the transition is not restricted.
            false
        })
    }
}

impl<N: Network + Serialize> Serialize for Restrictions<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Restrictions", 3)?;
        state.serialize_field("programs", &self.programs)?;
        state.serialize_field("functions", &self.functions)?;
        state.serialize_field("arguments", &self.arguments)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::types::I8;

    use indexmap::indexmap;
    use ledger_block::Input;

    type CurrentNetwork = console::network::MainnetV0;

    #[test]
    fn test_restrictions_program_restricted() {
        let mut restrictions = Restrictions::<CurrentNetwork>::default();
        let program_id = ProgramID::from_str("restricted.aleo").unwrap();
        let range = BlockRange::Range(10..20);
        restrictions.programs.insert(program_id, range);
        assert!(!restrictions.is_program_restricted(&program_id, 5));
        assert!(restrictions.is_program_restricted(&program_id, 10));
        assert!(restrictions.is_program_restricted(&program_id, 15));
        assert!(!restrictions.is_program_restricted(&program_id, 20));
        assert!(!restrictions.is_program_restricted(&program_id, 25));
    }

    #[test]
    fn test_restrictions_function_restricted() {
        let mut restrictions = Restrictions::<CurrentNetwork>::default();
        let program_id = ProgramID::from_str("restricted.aleo").unwrap();
        let function_id = Identifier::from_str("foo").unwrap();
        let range = BlockRange::Range(10..20);
        restrictions.functions.insert((program_id, function_id), range);
        assert!(!restrictions.is_function_restricted(&program_id, &function_id, 5));
        assert!(restrictions.is_function_restricted(&program_id, &function_id, 10));
        assert!(restrictions.is_function_restricted(&program_id, &function_id, 15));
        assert!(!restrictions.is_function_restricted(&program_id, &function_id, 20));
        assert!(!restrictions.is_function_restricted(&program_id, &function_id, 25));
    }

    #[test]
    fn test_restrictions_argument_restricted() {
        let rng = &mut TestRng::default();

        let mut restrictions = Restrictions::<CurrentNetwork>::default();
        let program_id = ProgramID::from_str("restricted.aleo").unwrap();
        let function_id = Identifier::from_str("bar").unwrap();
        let range = BlockRange::Range(10..20);

        let literal = Literal::I8(I8::new(42));
        let index = 0;
        restrictions
            .arguments
            .insert((program_id, function_id), indexmap!( (true, index) => indexmap!( literal.clone() => range )));

        let input = Input::Public(rng.gen(), Some(literal.into()));
        let transition =
            Transition::new(program_id, function_id, vec![input], vec![], rng.gen(), rng.gen(), rng.gen()).unwrap();
        assert!(!restrictions.is_argument_restricted(&transition, 5));
        assert!(restrictions.is_argument_restricted(&transition, 10));
        assert!(restrictions.is_argument_restricted(&transition, 15));
        assert!(!restrictions.is_argument_restricted(&transition, 20));
        assert!(!restrictions.is_argument_restricted(&transition, 25));
    }
}
