use std::time::{Duration, Instant};

use super::State;
use crate::utils::allocator::GsBuffer;
use smithay::backend::renderer::gles::GlesError;
use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer,
        damage::{Error as OutputDamageTrackerError, RenderOutputResult},
        element::{
            Kind, memory::MemoryRenderBufferRenderElement, surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::space::render_output,
    input::pointer::CursorImageStatus,
    render_elements,
};

pub const CURSOR_DATA_BYTES: &[u8] = include_bytes!("../../resources/cursor.rgba");

render_elements! {
    CursorElement<R> where R: Renderer + ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Memory=MemoryRenderBufferRenderElement<R>
}

impl State {
    pub fn create_frame(
        &mut self,
    ) -> Result<(gst::Buffer, RenderOutputResult), OutputDamageTrackerError<GlesError>> {
        assert!(self.output.is_some());
        assert!(self.dtr.is_some());
        assert!(self.video_info.is_some());
        assert!(self.output_buffer.is_some());

        let elements =
            if Instant::now().duration_since(self.last_pointer_movement) < Duration::from_secs(5) {
                match &self.cursor_state {
                CursorImageStatus::Named(_cursor_icon) => vec![CursorElement::Memory(
                    // TODO: icon?
                    MemoryRenderBufferRenderElement::from_buffer(
                        &mut self.renderer,
                        self.pointer_location.to_physical_precise_round(1),
                        &self.cursor_element,
                        None,
                        None,
                        None,
                        Kind::Cursor,
                    )
                    .map_err(OutputDamageTrackerError::Rendering)?,
                )],
                CursorImageStatus::Surface(wl_surface) => {
                    smithay::backend::renderer::element::surface::render_elements_from_surface_tree(
                        &mut self.renderer,
                        wl_surface,
                        self.pointer_location.to_physical_precise_round(1),
                        1.,
                        1.,
                        Kind::Cursor,
                    )
                }
                CursorImageStatus::Hidden => vec![],
            }
            } else {
                vec![]
            };

        let mut output_buffer = self.output_buffer.clone().expect("Output buffer not set");

        let mut target = output_buffer
            .bind(&mut self.renderer)
            .map_err(OutputDamageTrackerError::Rendering)?;

        let render_output_result = render_output(
            self.output.as_ref().unwrap(),
            &mut self.renderer,
            &mut target,
            1.0,
            0,
            [&self.space],
            &*elements,
            self.dtr.as_mut().unwrap(),
            [0.0, 0.0, 0.0, 1.0],
        )?;

        // Apply shader post-processing if configured
        #[cfg(feature = "shader")]
        if let Some(ref mut shader_state) = self.shader_state {
            let vi = self.video_info.as_ref().unwrap();
            let w = vi.width();
            let h = vi.height();
            if let Err(e) = Self::apply_shader(&self.renderer, shader_state, w, h) {
                tracing::warn!("Shader apply failed: {}, falling through without shader", e);
            }
        }

        match self
            .output_buffer
            .clone()
            .unwrap()
            .to_gs_buffer(&mut target, &mut self.renderer)
        {
            Ok(buffer) => Ok((buffer, render_output_result)),
            Err(e) => {
                tracing::warn!("Failed to convert buffer to gst buffer: {:?}", e);
                Err(OutputDamageTrackerError::Rendering(GlesError::MappingError))
            }
        }
    }

    /// Render a frame from the secondary output (second window in multi-output mode).
    /// This renders only the windows mapped to the secondary_space.
    pub fn create_secondary_frame(
        &mut self,
    ) -> Result<(gst::Buffer, RenderOutputResult), OutputDamageTrackerError<GlesError>> {
        assert!(self.secondary_output.is_some());
        assert!(self.secondary_dtr.is_some());
        assert!(self.secondary_video_info.is_some());
        assert!(self.secondary_output_buffer.is_some());

        // No cursor on secondary output (cursor stays on primary)
        let elements: Vec<CursorElement<_>> = vec![];

        let mut output_buffer = self
            .secondary_output_buffer
            .clone()
            .expect("Secondary output buffer not set");

        let mut target = output_buffer
            .bind(&mut self.renderer)
            .map_err(OutputDamageTrackerError::Rendering)?;

        let render_output_result = render_output(
            self.secondary_output.as_ref().unwrap(),
            &mut self.renderer,
            &mut target,
            1.0,
            0,
            [&self.secondary_space],
            &*elements,
            self.secondary_dtr.as_mut().unwrap(),
            [0.0, 0.0, 0.0, 1.0],
        )?;

        // Apply shader post-processing if configured
        #[cfg(feature = "shader")]
        if let Some(ref mut shader_state) = self.secondary_shader_state {
            let vi = self.secondary_video_info.as_ref().unwrap();
            let w = vi.width();
            let h = vi.height();
            if let Err(e) = Self::apply_shader(&self.renderer, shader_state, w, h) {
                tracing::warn!("Secondary shader apply failed: {}", e);
            }
        }

        match self
            .secondary_output_buffer
            .clone()
            .unwrap()
            .to_gs_buffer(&mut target, &mut self.renderer)
        {
            Ok(buffer) => Ok((buffer, render_output_result)),
            Err(e) => {
                tracing::warn!("Failed to convert secondary buffer to gst buffer: {:?}", e);
                Err(OutputDamageTrackerError::Rendering(GlesError::MappingError))
            }
        }
    }

    /// Apply the librashader filter chain to the currently-bound output FBO.
    ///
    /// Flow:
    /// 1. The output FBO already contains the composited frame (from render_output)
    /// 2. Blit output FBO → shader input FBO (captures frame for shader to read)
    /// 3. Run shader chain: input_tex → output_tex
    /// 4. Blit shader output FBO → output FBO (writes result back)
    ///
    /// This two-intermediate approach works with all buffer types (RAW, DMA, CUDA)
    /// because we operate purely at the GL level before to_gs_buffer() is called.
    #[cfg(feature = "shader")]
    fn apply_shader(
        renderer: &smithay::backend::renderer::gles::GlesRenderer,
        shader_state: &mut crate::shader::ShaderState,
        width: u32,
        height: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use smithay::backend::renderer::gles::ffi;

        // Ensure intermediate textures match current resolution
        shader_state.resize(width, height)?;

        let input_fbo_id = shader_state.input_fbo_id();
        let output_fbo_id = shader_state.output_fbo_id();

        // Step 1: Blit composited frame from the real output FBO → shader input FBO
        renderer
            .with_context(|gl| unsafe {
                // Get the currently-bound draw framebuffer (the output buffer's FBO)
                let mut real_fbo_id: i32 = 0;
                gl.GetIntegerv(ffi::DRAW_FRAMEBUFFER_BINDING, &mut real_fbo_id);
                let real_fbo_id = real_fbo_id as u32;

                // Blit: output FBO (READ) → shader input FBO (DRAW)
                gl.BindFramebuffer(ffi::READ_FRAMEBUFFER, real_fbo_id);
                gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, input_fbo_id);
                gl.BlitFramebuffer(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    ffi::COLOR_BUFFER_BIT,
                    ffi::NEAREST,
                );

                // Restore the real output FBO
                gl.BindFramebuffer(ffi::FRAMEBUFFER, real_fbo_id);
            })
            .map_err(|e| format!("blit to input failed: {:?}", e))?;

        // Step 2: Run librashader filter chain (input_tex → output_tex)
        shader_state.apply()?;

        // Step 3: Blit shader output FBO → real output FBO
        renderer
            .with_context(|gl| unsafe {
                let mut real_fbo_id: i32 = 0;
                gl.GetIntegerv(ffi::DRAW_FRAMEBUFFER_BINDING, &mut real_fbo_id);
                let real_fbo_id = real_fbo_id as u32;

                // Blit: shader output FBO (READ) → real output FBO (DRAW)
                gl.BindFramebuffer(ffi::READ_FRAMEBUFFER, output_fbo_id);
                gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, real_fbo_id);
                gl.BlitFramebuffer(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    ffi::COLOR_BUFFER_BIT,
                    ffi::NEAREST,
                );

                // Restore the real output FBO
                gl.BindFramebuffer(ffi::FRAMEBUFFER, real_fbo_id);
            })
            .map_err(|e| format!("blit from output failed: {:?}", e))?;

        Ok(())
    }
}
