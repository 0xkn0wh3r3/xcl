//! Test and diagnostic oracles from the Windows SDK: DXC's validator
//! (`IDxcValidator` from `dxcompiler.dll`, which carries the internal
//! validator since DXC 1.8 — the SDK's `dxil.dll` copies may be older) and
//! its disassembler. Emitted shaders do
//! not need either: their container digest is computed by
//! [`super::container::hash`].

use std::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::Dxc::*;

type CreateInstance = unsafe extern "system" fn(*const windows::core::GUID, *const windows::core::GUID, *mut *mut c_void) -> windows::core::HRESULT;

fn compiler_create() -> Result<CreateInstance, String> {
    let f = crate::gpu::recomp::dxc::create_instance().ok_or("dxcompiler.dll not found")?;
    Ok(unsafe { std::mem::transmute::<usize, CreateInstance>(f) })
}

fn instance<T: Interface>(create: CreateInstance, clsid: &windows::core::GUID) -> Result<T, String> {
    let mut p = std::ptr::null_mut();
    let hr = unsafe { create(clsid, &T::IID, &mut p) };
    if hr.is_err() || p.is_null() {
        return Err(format!("DxcCreateInstance: {hr:?}"));
    }
    Ok(unsafe { T::from_raw(p) })
}

fn blob_text(b: &IDxcBlob) -> String {
    unsafe {
        let (p, n) = (b.GetBufferPointer() as *const u8, b.GetBufferSize());
        // An empty blob may have a null buffer, which `from_raw_parts` forbids.
        if p.is_null() || n == 0 {
            return String::new();
        }
        String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).trim_end_matches('\0').to_string()
    }
}

/// The text of an operation's error buffer (empty if there is none).
fn error_text(e: windows::core::Result<IDxcBlobEncoding>) -> String {
    e.ok().and_then(|e| e.cast::<IDxcBlob>().ok()).map(|b| blob_text(&b)).unwrap_or_default()
}

fn blob(bytes: &[u8]) -> Result<IDxcBlob, String> {
    let utils: IDxcUtils = instance(compiler_create()?, &CLSID_DxcUtils)?;
    unsafe {
        let len = u32::try_from(bytes.len()).map_err(|_| "blob larger than 4 GiB".to_string())?;
        let b = utils.CreateBlob(bytes.as_ptr() as *const c_void, len, DXC_CP_ACP).map_err(|e| format!("CreateBlob: {e}"))?;
        b.cast::<IDxcBlob>().map_err(|e| format!("{e}"))
    }
}

/// Validates a container with DXC's validator; `Err` carries the validator's messages.
pub fn validate(container: &[u8]) -> Result<(), String> {
    let v: IDxcValidator = instance(compiler_create()?, &CLSID_DxcValidator)?;
    let b = blob(container)?;
    unsafe {
        let r = v.Validate(&b, DxcValidatorFlags_Default).map_err(|e| format!("Validate: {e}"))?;
        let status = r.GetStatus().map_err(|e| format!("{e}"))?;
        if status.is_ok() {
            return Ok(());
        }
        let msg = error_text(r.GetErrorBuffer());
        Err(if msg.is_empty() { format!("validation failed: {status:?}") } else { msg })
    }
}

/// DXC's textual disassembly of a container (for diagnostics and tests).
pub fn disassemble(container: &[u8]) -> Result<String, String> {
    let c: IDxcCompiler3 = instance(compiler_create()?, &CLSID_DxcCompiler)?;
    let buf = DxcBuffer { Ptr: container.as_ptr() as *const c_void, Size: container.len(), Encoding: 0 };
    unsafe {
        let r: IDxcResult = c.Disassemble(&buf).map_err(|e| format!("Disassemble: {e}"))?;
        let status = r.GetStatus().map_err(|e| format!("{e}"))?;
        if status.is_err() {
            let msg = error_text(r.GetErrorBuffer());
            return Err(format!("disassembly failed: {msg}"));
        }
        let out: IDxcBlob = r.GetResult().map_err(|e| format!("{e}"))?;
        Ok(blob_text(&out))
    }
}

#[cfg(test)]
mod tests {
    /// Malformed input must come back as `Err`, never as a panic or UB in the
    /// FFI glue (empty error blobs may carry a null buffer pointer).
    #[test]
    fn malformed_containers_are_errors() {
        let s = super::super::shader::Shader::new(super::super::container::Stage::Compute, (6, 6));
        let good = s.finish([1, 1, 1], None);
        assert_eq!(super::validate(&good), Ok(()));
        // Garbage and truncations. (A wrong digest is *not* an error here:
        // the validator re-signs rather than checks it; D3D12 does check it,
        // which the GPU tests cover.)
        let inputs: [&[u8]; 5] = [&[], b"DXBC", &[0u8; 64], &good[..good.len() / 2], &good[..good.len() - 4]];
        for c in inputs {
            if crate::gpu::recomp::dxc::create_instance().is_none() {
                return crate::gpu::recomp::runner::skip_test("no dxcompiler.dll");
            }
            assert!(super::validate(c).is_err(), "accepted {} bytes", c.len());
            let _ = super::disassemble(c);
        }
    }
}
