//!
//! Manage vertex and index buffers of single objects with ease.
//!

use std::borrow::Cow;
use std::mem::size_of;

use hal::{Backend, IndexCount, IndexType, Primitive, VertexCount};
use hal::buffer::{Access, IndexBufferView, Usage};
use hal::command::RenderSubpassCommon;
use hal::memory::Properties;
use hal::pso::VertexBufferSet;

use smallvec::SmallVec;

use render::{Buffer, Error, Factory};
use utils::{cast_cow, is_slice_sorted, is_slice_sorted_by_key};
use vertex::{AsVertexFormat, VertexFormat};

/// Vertex buffer with it's format
#[derive(Debug)]
pub struct VertexBuffer<B: Backend> {
    buffer: Buffer<B>,
    format: VertexFormat<'static>,
    len: VertexCount,
}

/// Index buffer with it's type
#[derive(Debug)]
pub struct IndexBuffer<B: Backend> {
    buffer: Buffer<B>,
    index_type: IndexType,
    len: IndexCount,
}

/// Abstracts over two types of indices and their absence.
#[derive(Debug)]
pub enum Indices<'a> {
    /// No indices.
    None,

    /// `u16` per index.
    U16(Cow<'a, [u16]>),

    /// `u32` per index.
    U32(Cow<'a, [u32]>),
}

impl From<Vec<u16>> for Indices<'static> {
    fn from(vec: Vec<u16>) -> Self {
        Indices::U16(vec.into())
    }
}

impl<'a> From<&'a [u16]> for Indices<'a> {
    fn from(slice: &'a [u16]) -> Self {
        Indices::U16(slice.into())
    }
}

impl<'a> From<Cow<'a, [u16]>> for Indices<'a> {
    fn from(cow: Cow<'a, [u16]>) -> Self {
        Indices::U16(cow)
    }
}

impl From<Vec<u32>> for Indices<'static> {
    fn from(vec: Vec<u32>) -> Self {
        Indices::U32(vec.into())
    }
}

impl<'a> From<&'a [u32]> for Indices<'a> {
    fn from(slice: &'a [u32]) -> Self {
        Indices::U32(slice.into())
    }
}

impl<'a> From<Cow<'a, [u32]>> for Indices<'a> {
    fn from(cow: Cow<'a, [u32]>) -> Self {
        Indices::U32(cow)
    }
}

/// Generics-free mesh builder.
/// Useful for creating mesh from non-predefined set of data.
/// Like from glTF.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MeshBuilder<'a> {
    vertices: SmallVec<[(Cow<'a, [u8]>, VertexFormat<'static>); 16]>,
    indices: Option<(Cow<'a, [u8]>, IndexType)>,
    prim: Primitive,
}

impl<'a> MeshBuilder<'a> {
    /// Create empty builder.
    pub fn new() -> Self {
        MeshBuilder {
            vertices: SmallVec::new(),
            indices: None,
            prim: Primitive::TriangleList,
        }
    }

    /// Set indices buffer to the `MeshBuilder`
    pub fn with_indices<I>(mut self, indices: I) -> Self
    where
        I: Into<Indices<'a>>,
    {
        self.set_indices(indices);
        self
    }

    /// Set indices buffer to the `MeshBuilder`
    pub fn set_indices<I>(&mut self, indices: I) -> &mut Self
    where
        I: Into<Indices<'a>>,
    {
        self.indices = match indices.into() {
            Indices::None => None,
            Indices::U16(i) => Some((cast_cow(i), IndexType::U16)),
            Indices::U32(i) => Some((cast_cow(i), IndexType::U32)),
        };
        self
    }

    /// Add another vertices to the `MeshBuilder`
    pub fn with_vertices<V, D>(mut self, vertices: D) -> Self
    where
        V: AsVertexFormat + 'a,
        D: Into<Cow<'a, [V]>>,
    {
        self.add_vertices(vertices);
        self
    }

    /// Add another vertices to the `MeshBuilder`
    pub fn add_vertices<V, D>(&mut self, vertices: D) -> &mut Self
    where
        V: AsVertexFormat + 'a,
        D: Into<Cow<'a, [V]>>,
    {
        self.vertices
            .push((cast_cow(vertices.into()), V::VERTEX_FORMAT));
        self
    }

    /// Sets the primitive type of the mesh.
    ///
    /// By default, meshes are constructed as triangle lists.
    pub fn with_prim_type(mut self, prim: Primitive) -> Self {
        self.prim = prim;
        self
    }

    /// Sets the primitive type of the mesh.
    ///
    /// By default, meshes are constructed as triangle lists.
    pub fn set_prim_type(&mut self, prim: Primitive) -> &mut Self {
        self.prim = prim;
        self
    }

    /// Builds and returns the new mesh.
    pub fn build<B>(&self, factory: &mut Factory<B>) -> Result<Mesh<B>, Error>
    where
        B: Backend,
    {
        Ok(Mesh {
            vbufs: self.vertices
                .iter()
                .map(|&(ref vertices, ref format)| {
                    let len = vertices.len() as VertexCount / format.stride;
                    Ok(VertexBuffer {
                        buffer: {
                            let mut buffer = factory.create_buffer(
                                vertices.len() as _,
                                Properties::DEVICE_LOCAL,
                                Usage::VERTEX | Usage::TRANSFER_DST,
                            )?;
                            factory.upload_buffer(
                                Access::VERTEX_BUFFER_READ,
                                &mut buffer,
                                0,
                                &vertices,
                            )?;
                            buffer
                        },
                        format: format.clone(),
                        len,
                    })
                })
                .collect::<Result<_, Error>>()?,
            ibuf: match self.indices {
                None => None,
                Some((ref indices, index_type)) => {
                    let stride = match index_type {
                        IndexType::U16 => size_of::<u16>(),
                        IndexType::U32 => size_of::<u32>(),
                    };
                    let len = indices.len() as IndexCount / stride as IndexCount;
                    Some(IndexBuffer {
                        buffer: {
                            let mut buffer = factory.create_buffer(
                                indices.len() as _,
                                Properties::DEVICE_LOCAL,
                                Usage::INDEX | Usage::TRANSFER_DST,
                            )?;
                            factory.upload_buffer(
                                Access::INDEX_BUFFER_READ,
                                &mut buffer,
                                0,
                                &indices,
                            )?;
                            buffer
                        },
                        index_type,
                        len,
                    })
                }
            },
            prim: self.prim,
        })
    }
}

/// Single mesh is a collection of buffers that provides available attributes.
/// Exactly one mesh is used per drawing call in common.
#[derive(Debug)]
pub struct Mesh<B: Backend> {
    vbufs: Vec<VertexBuffer<B>>,
    ibuf: Option<IndexBuffer<B>>,
    prim: Primitive,
}

impl<B> Mesh<B>
where
    B: Backend,
{
    /// Build new mesh with `HMeshBuilder`
    pub fn new<'a>() -> MeshBuilder<'a> {
        MeshBuilder::new()
    }

    /// Primitive type of the `Mesh`
    pub fn primitive(&self) -> Primitive {
        self.prim
    }

    /// Bind buffers to specified attribute locations.
    pub fn bind<'a>(
        &'a self,
        formats: &'a [VertexFormat<'a>],
        vertex: &mut VertexBufferSet<'a, B>,
    ) -> Result<Bind<'a, B>, Incompatible> {
        debug_assert!(is_slice_sorted(formats));
        debug_assert!(is_slice_sorted_by_key(&self.vbufs, |vbuf| {
            &vbuf.format
        }));
        debug_assert!(vertex.0.is_empty());

        let mut next = 0;
        let mut vertex_count = None;
        for format in formats {
            if let Some(index) = find_compatible_buffer(&self.vbufs[next..], format) {
                // Ensure buffer is valid
                vertex.0.push((self.vbufs[index].buffer.raw(), 0));
                next = index + 1;
                assert!(vertex_count.is_none() || vertex_count == Some(self.vbufs[index].len));
                vertex_count = Some(self.vbufs[index].len);
            } else {
                // Can't bind
                return Err(Incompatible);
            }
        }
        Ok(self.ibuf
            .as_ref()
            .map(|ibuf| Bind::Indexed {
                index: IndexBufferView {
                    buffer: ibuf.buffer.raw(),
                    offset: 0,
                    index_type: ibuf.index_type,
                },
                count: ibuf.len,
            })
            .unwrap_or(Bind::Unindexed {
                count: vertex_count.unwrap_or(0),
            }))
    }

    /// Destroy `Mesh`.
    pub fn dispose(self, factory: &mut Factory<B>) {
        if let Some(ibuf) = self.ibuf {
            factory.destroy_buffer(ibuf.buffer);
        }

        for vbuf in self.vbufs {
            factory.destroy_buffer(vbuf.buffer);
        }
    }
}

/// Error type returned by `Mesh::bind` in case of mesh's vertex buffers are incompatible with requested vertex formats.
#[derive(Clone, Copy, Debug)]
pub struct Incompatible;

/// Result of buffers bindings.
/// It only contains `IndexBufferView` (if index buffers exists)
/// and vertex count.
/// Vertex buffers are in separate `VertexBufferSet`
// #[derive(Debug)]
pub enum Bind<'a, B: Backend> {
    /// Indexed binding.
    Indexed {
        /// Index view to bind with `bind_index_buffer` method.
        index: IndexBufferView<'a, B>,
        /// Indices count to use in `draw_indexed` method.
        count: IndexCount,
    },
    /// Not indexed binding.
    Unindexed {
        /// Vertex count to use in `draw` method.
        count: VertexCount,
    },
}

impl<'a, B> Bind<'a, B>
where
    B: Backend,
{
    /// Record drawing command for this biding.
    pub fn draw(self, vertex: VertexBufferSet<B>, encoder: &mut RenderSubpassCommon<B>) {
        encoder.bind_vertex_buffers(vertex);
        match self {
            Bind::Indexed { index, count } => {
                encoder.bind_index_buffer(index);
                encoder.draw_indexed(0..count, 0, 0..1);
            }
            Bind::Unindexed { count } => {
                encoder.draw(0..count, 0..1);
            }
        }
    }
}

/// Helper function to find buffer with compatible format.
fn find_compatible_buffer<B>(vbufs: &[VertexBuffer<B>], format: &VertexFormat) -> Option<usize>
where
    B: Backend,
{
    debug_assert!(is_slice_sorted_by_key(&*format.attributes, |a| {
        a.offset
    }));
    for (i, vbuf) in vbufs.iter().enumerate() {
        debug_assert!(is_slice_sorted_by_key(&*vbuf.format.attributes, |a| {
            a.offset
        }));
        if is_compatible(&vbuf.format, format) {
            return Some(i);
        }
    }
    None
}

/// Check is vertex format `left` is compatible with `right`.
/// `left` must have same `stride` and contain all attributes from `right`.
fn is_compatible(left: &VertexFormat, right: &VertexFormat) -> bool {
    if left.stride != right.stride {
        return false;
    }

    // Don't start searching from index 0 because attributes are sorted
    let mut skip = 0;
    right.attributes.iter().all(|r| {
        left.attributes[skip..]
            .iter()
            .position(|l| *l == *r)
            .map_or(false, |p| {
                skip += p;
                true
            })
    })
}
