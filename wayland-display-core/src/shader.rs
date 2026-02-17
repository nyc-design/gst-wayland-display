//! RetroArch-compatible shader pipeline using librashader.
//!
//! Integrates the librashader OpenGL runtime into the Smithay compositor's
//! render loop, applying .slangp shader presets to the composited frame
//! before it is exported as a GStreamer buffer.
//!
//! The shader pass sits between `render_output()` and `to_gs_buffer()`,
//! making it completely emulator-agnostic — any Wayland client's output
//! gets the shader treatment.
//!
//! ## GL Pipeline
//!
//! After `render_output()` composites into the output FBO:
//! 1. Blit output FBO → input FBO (captures composited frame into `input_tex`)
//! 2. librashader `frame()`: reads `input_tex`, writes into `output_tex`
//! 3. Blit output FBO ← shader output FBO (copies shader result back)
//!
//! This two-intermediate approach works with all buffer types (RAW, DMA, CUDA)
//! because we operate purely at the GL level before `to_gs_buffer()`.

use glow::HasContext;
use librashader::presets::{ShaderPreset, context::WildcardContext};
use librashader::runtime::gl::{FilterChainGL, GLImage};
use librashader::runtime::{FilterChainParameters, Viewport};
use std::path::Path;
use std::sync::Arc;

/// Holds the librashader filter chain and associated GL resources.
pub struct ShaderState {
    /// The librashader filter chain (multi-pass shader pipeline).
    chain: FilterChainGL,
    /// glow context wrapping the EGL proc address loader.
    ctx: Arc<glow::Context>,

    /// Input texture: composited frame is blitted here from the output FBO.
    /// librashader reads from this texture.
    input_tex: glow::Texture,
    input_fbo: glow::Framebuffer,

    /// Output texture: librashader writes shader result here.
    /// We then blit this back to the real output FBO.
    output_tex: glow::Texture,
    output_fbo: glow::Framebuffer,

    /// Current resolution of the intermediate textures.
    width: u32,
    height: u32,
    /// Running frame counter (passed to librashader for time-based effects).
    frame_count: usize,
}

/// Create a texture + FBO pair at the given resolution.
unsafe fn create_tex_fbo(
    ctx: &glow::Context,
    width: u32,
    height: u32,
    label: &str,
) -> Result<(glow::Texture, glow::Framebuffer), Box<dyn std::error::Error>> {
    let tex = ctx
        .create_texture()
        .map_err(|e| format!("{} create_texture: {}", label, e))?;
    ctx.bind_texture(glow::TEXTURE_2D, Some(tex));
    ctx.tex_storage_2d(
        glow::TEXTURE_2D,
        1,
        glow::RGBA8,
        width as i32,
        height as i32,
    );
    ctx.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::LINEAR as i32,
    );
    ctx.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::LINEAR as i32,
    );
    ctx.bind_texture(glow::TEXTURE_2D, None);

    let fbo = ctx
        .create_framebuffer()
        .map_err(|e| format!("{} create_framebuffer: {}", label, e))?;
    ctx.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    ctx.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::COLOR_ATTACHMENT0,
        glow::TEXTURE_2D,
        Some(tex),
        0,
    );
    let status = ctx.check_framebuffer_status(glow::FRAMEBUFFER);
    if status != glow::FRAMEBUFFER_COMPLETE {
        return Err(format!("{} FBO incomplete: 0x{:x}", label, status).into());
    }
    ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
    Ok((tex, fbo))
}

/// Convert a glow::Framebuffer to a raw u32 GL name.
fn fbo_to_raw(fbo: glow::Framebuffer) -> u32 {
    fbo.0.get()
}

impl ShaderState {
    /// Create a new shader state from a `.slangp` preset file.
    ///
    /// `gl_proc_fn` should return OpenGL function pointers (typically from
    /// `eglGetProcAddress`). The GL context must be current on this thread.
    ///
    /// `param_overrides` is a list of `("PARAM_NAME", value)` pairs that
    /// override default parameter values in the preset.
    pub fn new(
        preset_path: &str,
        param_overrides: &[(String, f32)],
        gl_proc_fn: impl Fn(&str) -> *const std::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        tracing::info!("Loading shader preset: {}", preset_path);

        // Parse the .slangp preset
        let preset = ShaderPreset::try_parse_with_driver_context(
            Path::new(preset_path),
            WildcardContext::new(),
        )?;

        // Create glow context from the EGL proc address loader
        let ctx =
            unsafe { Arc::new(glow::Context::from_loader_function(|name| gl_proc_fn(name))) };

        // Create the librashader filter chain (auto-detects GLES vs desktop GL)
        let options = librashader::runtime::gl::FilterChainOptionsGL {
            glsl_version: 0,  // auto-detect from context
            use_dsa: false,   // GLES 3.x compatible (no DSA)
            force_no_mipmaps: false,
            disable_cache: false,
        };
        let mut chain = unsafe {
            FilterChainGL::load_from_preset(preset, Arc::clone(&ctx), Some(&options))?
        };

        // Apply parameter overrides
        for (name, value) in param_overrides {
            chain.set_parameter(name, *value);
        }

        // Create two intermediate texture+FBO pairs:
        //   input:  composited frame is blitted here, librashader reads it
        //   output: librashader writes shader result here, we blit back
        let (input_tex, input_fbo) =
            unsafe { create_tex_fbo(&ctx, width, height, "shader_input")? };
        let (output_tex, output_fbo) =
            unsafe { create_tex_fbo(&ctx, width, height, "shader_output")? };

        tracing::info!(
            "Shader pipeline initialized: {}x{}, {} passes",
            width,
            height,
            chain.get_active_pass_count(),
        );

        Ok(ShaderState {
            chain,
            ctx,
            input_tex,
            input_fbo,
            output_tex,
            output_fbo,
            width,
            height,
            frame_count: 0,
        })
    }

    /// Returns the raw GL framebuffer ID of the input FBO.
    pub fn input_fbo_id(&self) -> u32 {
        fbo_to_raw(self.input_fbo)
    }

    /// Returns the raw GL framebuffer ID of the output FBO.
    pub fn output_fbo_id(&self) -> u32 {
        fbo_to_raw(self.output_fbo)
    }

    /// Apply the shader chain.
    ///
    /// Reads from `input_tex` (which should already contain the composited
    /// frame via a blit), processes through the shader chain, and writes
    /// the result into `output_tex`.
    pub fn apply(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let input = GLImage {
            handle: Some(self.input_tex),
            format: glow::RGBA8,
            size: librashader::runtime::Size::new(self.width, self.height),
        };

        let output_image = GLImage {
            handle: Some(self.output_tex),
            format: glow::RGBA8,
            size: librashader::runtime::Size::new(self.width, self.height),
        };

        let viewport = Viewport::new_render_target_sized_origin(&output_image, None)?;

        unsafe {
            self.chain
                .frame(&input, &viewport, self.frame_count, None)?;
        }

        self.frame_count = self.frame_count.wrapping_add(1);
        Ok(())
    }

    /// Resize intermediate textures and FBOs when the output resolution changes.
    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.width == width && self.height == height {
            return Ok(());
        }
        tracing::info!(
            "Resizing shader intermediates: {}x{} -> {}x{}",
            self.width,
            self.height,
            width,
            height
        );

        unsafe {
            // Delete old resources
            self.ctx.delete_framebuffer(self.input_fbo);
            self.ctx.delete_texture(self.input_tex);
            self.ctx.delete_framebuffer(self.output_fbo);
            self.ctx.delete_texture(self.output_tex);

            // Create new ones at the new size
            let (input_tex, input_fbo) =
                create_tex_fbo(&self.ctx, width, height, "shader_input")?;
            let (output_tex, output_fbo) =
                create_tex_fbo(&self.ctx, width, height, "shader_output")?;

            self.input_tex = input_tex;
            self.input_fbo = input_fbo;
            self.output_tex = output_tex;
            self.output_fbo = output_fbo;
        }

        self.width = width;
        self.height = height;
        Ok(())
    }
}

impl Drop for ShaderState {
    fn drop(&mut self) {
        unsafe {
            self.ctx.delete_framebuffer(self.input_fbo);
            self.ctx.delete_texture(self.input_tex);
            self.ctx.delete_framebuffer(self.output_fbo);
            self.ctx.delete_texture(self.output_tex);
        }
        tracing::info!("Shader pipeline destroyed");
    }
}

/// Parse shader parameter overrides from a semicolon-separated string.
///
/// Format: `"PARAM1=0.5;PARAM2=1.0;PARAM3=2.2"`
pub fn parse_shader_params(params_str: &str) -> Vec<(String, f32)> {
    if params_str.is_empty() {
        return Vec::new();
    }
    params_str
        .split(';')
        .filter_map(|pair| {
            let (key, val) = pair.split_once('=')?;
            let value: f32 = val.trim().parse().ok()?;
            Some((key.trim().to_string(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shader_params() {
        let params = parse_shader_params("GRID_STRENGTH=0.08;gamma=2.2;blur=1.5");
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], ("GRID_STRENGTH".to_string(), 0.08));
        assert_eq!(params[1], ("gamma".to_string(), 2.2));
        assert_eq!(params[2], ("blur".to_string(), 1.5));
    }

    #[test]
    fn test_parse_shader_params_empty() {
        let params = parse_shader_params("");
        assert!(params.is_empty());
    }

    #[test]
    fn test_parse_shader_params_invalid() {
        let params = parse_shader_params("valid=1.0;invalid;also_invalid=abc;ok=2.0");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], ("valid".to_string(), 1.0));
        assert_eq!(params[1], ("ok".to_string(), 2.0));
    }
}
