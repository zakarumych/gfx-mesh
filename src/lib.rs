//!
//! This crates provides means to deal with vertex buffers and meshes.
//! 
//! `Attribute` and `VertexFormat` allow vertex structure to declare semantics.
//! `Mesh` can be created from typed vertex structures and provides mechanism to bind
//! vertex attributes required by shader interface.
//!
#![deny(missing_docs)]
#![deny(dead_code)]
#![deny(unused_must_use)]

extern crate failure;
extern crate gfx_hal as hal;
extern crate gfx_render as render;

#[cfg(feature="serde")]
#[macro_use]
extern crate serde;
extern crate smallvec;

mod mesh;
mod utils;
mod vertex;

pub use vertex::{AsVertexFormat, Attribute, Color, Normal, PosColor, PosNormTangTex, PosNormTex,
                 PosTex, Position, Query, Tangent, TexCoord, VertexFormat, WithAttribute};
pub use mesh::{Bind, Incompatible, IndexBuffer, Indices, Mesh, MeshBuilder, VertexBuffer};
