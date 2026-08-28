use cranelift_codegen::ir::{types, AbiParam, InstBuilder};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::error::Error;
use std::path::Path;

/// Milestone-0 spike: builds an object file for a `main()` that boxes
/// `text` via `kl_str_new` and prints it via `kl_print_text(*mut u8)`
/// (both defined in kolom-runtime — kept in sync with the real M2 `kl_str`
/// heap representation so this still links against current kolom-runtime).
/// No C source is generated and no C compiler is invoked anywhere in this
/// function — Cranelift emits real machine code directly.
pub fn build_hello_object(out_path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    let mut flag_builder = settings::builder();
    flag_builder.set("is_pic", "false")?;
    let isa_builder = crate::link::isa_builder()?;
    let isa = isa_builder.finish(settings::Flags::new(flag_builder))?;

    let obj_builder =
        ObjectBuilder::new(isa, "kolom_hello", cranelift_module::default_libcall_names())?;
    let mut module = ObjectModule::new(obj_builder);
    let ptr_ty = module.target_config().pointer_type();

    // extern "C" fn kl_str_new(bytes: *const u8, len: i64) -> *mut u8
    let mut str_new_sig = module.make_signature();
    str_new_sig.params.push(AbiParam::new(ptr_ty));
    str_new_sig.params.push(AbiParam::new(types::I64));
    str_new_sig.returns.push(AbiParam::new(ptr_ty));
    let str_new_func_id = module.declare_function("kl_str_new", Linkage::Import, &str_new_sig)?;

    // extern "C" fn kl_print_text(p: *mut u8)
    let mut print_sig = module.make_signature();
    print_sig.params.push(AbiParam::new(ptr_ty));
    let print_func_id = module.declare_function("kl_print_text", Linkage::Import, &print_sig)?;

    // static bytes for the text literal
    let mut data_desc = DataDescription::new();
    data_desc.define(text.as_bytes().to_vec().into_boxed_slice());
    let data_id = module.declare_data("hello_str", Linkage::Local, false, false)?;
    module.define_data(data_id, &data_desc)?;

    // fn main() -> i32 { kl_print_text(&hello_str, len); return 0; }
    let mut main_sig = module.make_signature();
    main_sig.returns.push(AbiParam::new(types::I32));
    let main_func_id = module.declare_function("main", Linkage::Export, &main_sig)?;

    let mut ctx = module.make_context();
    ctx.func.signature = main_sig;
    let mut fn_builder_ctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fn_builder_ctx);
        let block = builder.create_block();
        builder.switch_to_block(block);
        builder.seal_block(block);

        let local_str_new = module.declare_func_in_func(str_new_func_id, builder.func);
        let local_print = module.declare_func_in_func(print_func_id, builder.func);
        let local_data = module.declare_data_in_func(data_id, builder.func);
        let data_ptr = builder.ins().symbol_value(ptr_ty, local_data);
        let len = builder.ins().iconst(types::I64, text.as_bytes().len() as i64);
        let new_call = builder.ins().call(local_str_new, &[data_ptr, len]);
        let str_ptr = builder.inst_results(new_call)[0];
        builder.ins().call(local_print, &[str_ptr]);
        let zero = builder.ins().iconst(types::I32, 0);
        builder.ins().return_(&[zero]);
        let frontend_config = module.target_config();
        builder.finalize(frontend_config);
    }
    module.define_function(main_func_id, &mut ctx)?;
    module.clear_context(&mut ctx);

    let product = module.finish();
    let bytes = product.object.write()?;
    std::fs::write(out_path, bytes)?;
    Ok(())
}
