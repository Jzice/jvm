use crate::classfile::constant_pool::ConstantType;
use crate::classfile::consts;
use crate::classfile::types::*;
use crate::classfile::ClassFile;
use crate::oop::{self, consts as oop_consts, field, ClassRef, Method, Oop, OopDesc, ValueType, MethodIdRef};
use crate::runtime::thread::JavaThread;
use crate::runtime::{self, JavaThreadRef, Local, Stack};
use bytes::{BigEndian, Bytes};
use std::borrow::BorrowMut;
use std::hint::unreachable_unchecked;
use std::ops::Deref;
use std::sync::Arc;

pub struct Frame {
    thread: JavaThreadRef,
    method: MethodIdRef,
    local: Local,
    stack: Stack,
    pc: i32,
    class: ClassRef,
    code: Arc<Vec<U1>>,
    return_v: Option<Arc<OopDesc>>,
}

//new & helper methods
impl Frame {
    pub fn new(thread: JavaThreadRef, class: ClassRef, method: MethodIdRef) -> Self {
        let local = Local::new(method.method.code.max_locals as usize);
        let stack = Stack::new(method.method.code.max_stack as usize);
        let code = method.method.code.code.clone();
        Self {
            thread,
            method,
            local,
            stack,
            pc: 0,
            class,
            code,
            return_v: None,
        }
    }

    fn read_i1(&mut self) -> i32 {
        let v = self.code[self.pc as usize];
        self.pc += 1;
        v as i32
    }

    fn read_i2(&mut self) -> i32 {
        self.read_i1() << 8 | self.read_i1()
    }

    fn read_i4(&mut self) -> i32 {
        self.read_i2() << 16 | self.read_i2()
    }

    fn read_u1(&mut self) -> usize {
        let v = self.code[self.pc as usize];
        self.pc += 1;
        v as usize
    }

    fn read_u2(&mut self) -> usize {
        self.read_u1() << 8 | self.read_u1()
    }

    fn load_constant(&mut self, pos: usize) {
        let cp = &self.class.lock().unwrap().class_file.cp;

        match &cp[pos] {
            ConstantType::Integer { v } => self.stack.push_int2(*v),
            ConstantType::Float { v } => self.stack.push_float2(*v),
            ConstantType::Long { v } => self.stack.push_long2(*v),
            ConstantType::Double { v } => self.stack.push_double2(*v),
            ConstantType::String { string_index } => {
                if let ConstantType::Utf8 { length, bytes } = &cp[*string_index as usize] {
                    self.stack.push_const_utf8(bytes.clone());
                } else {
                    unreachable!()
                }
            }
            ConstantType::Class { name_index } => {
                //todo: impl me
                unimplemented!()
            }
            _ => unreachable!(),
        }
    }

    fn handle_exception(&mut self) {
        self.stack.clear();
        let ext = self.thread.exception.as_ref();
        self.stack.push_ref(ext.unwrap().clone());
        let mut thread = self.thread.clone();
        JavaThread::clear_ext(thread);
        self.athrow();
    }

    fn goto_abs(&mut self, pc: i32) {
        self.pc = pc;
    }

    fn set_return(&mut self, v: Arc<OopDesc>) {
        self.return_v = Some(v);
    }

    fn get_field_helper(&self, receiver: Arc<OopDesc>, idx: i32) -> (Arc<OopDesc>, ValueType) {
        let is_static = Arc::ptr_eq(&receiver, &oop_consts::get_null());
        let thread = self.thread.clone();

        let field_id = {
            let mut class = self.class.lock().unwrap();
            let cp = &class.class_file.cp;
            field::get_field_ref(thread, cp, idx as usize, is_static)
        };

        let value_type = field_id.field.value_type.clone();
        let class = field_id.field.class.lock().unwrap();
        let v = if is_static {
            class.get_static_field_value(field_id.clone())
        } else {
            class.get_field_value(receiver, field_id.clone())
        };

        (v, value_type)
    }

    fn put_field_helper(&mut self, idx: i32, is_static: bool) {
        let thread = self.thread.clone();

        let field_id = {
            let mut class = self.class.lock().unwrap();
            let cp = &class.class_file.cp;
            field::get_field_ref(thread, cp, idx as usize, is_static)
        };

        let value_type = field_id.field.value_type.clone();

        let v = match value_type {
            ValueType::ARRAY | ValueType::OBJECT => self.stack.pop_ref(),
            ValueType::INT
            | ValueType::SHORT
            | ValueType::CHAR
            | ValueType::BOOLEAN
            | ValueType::BYTE => {
                let v = self.stack.pop_int();
                OopDesc::new_int(v)
            }
            ValueType::FLOAT => {
                let v = self.stack.pop_float();
                OopDesc::new_float(v)
            }
            ValueType::DOUBLE => {
                let v = self.stack.pop_double();
                OopDesc::new_double(v)
            }
            ValueType::LONG => {
                let v = self.stack.pop_long();
                OopDesc::new_long(v)
            }
            _ => unreachable!(),
        };

        let mut class = field_id.field.class.lock().unwrap();
        if is_static {
            class.put_static_field_value(field_id.clone(), v);
        } else {
            let receiver = self.stack.pop_ref();
            if Arc::ptr_eq(&receiver, &oop_consts::get_null()) {
                let thread = self.thread.clone();
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            } else {
                class.put_field_value(receiver, field_id.clone(), v);
            }
        }
    }
}

//byte code impl
impl Frame {
    pub fn nop(&mut self) {}

    pub fn aconst_null(&mut self) {
        self.stack.push_null();
    }

    pub fn iconst_m1(&mut self) {
        self.stack.push_const_m1();
    }

    pub fn iconst_0(&mut self) {
        self.stack.push_const0();
    }

    pub fn lconst_0(&mut self) {
        self.stack.push_const0();
    }

    pub fn fconst_0(&mut self) {
        self.stack.push_const0();
    }

    pub fn dconst_0(&mut self) {
        self.stack.push_const0();
    }

    pub fn iconst_1(&mut self) {
        self.stack.push_const1();
    }

    pub fn lconst_1(&mut self) {
        self.stack.push_const1();
    }

    pub fn fconst_1(&mut self) {
        self.stack.push_const1();
    }

    pub fn dconst_1(&mut self) {
        self.stack.push_const1();
    }

    pub fn iconst_2(&mut self) {
        self.stack.push_const2();
    }

    pub fn fconst_2(&mut self) {
        self.stack.push_const2();
    }

    pub fn iconst_3(&mut self) {
        self.stack.push_const3();
    }

    pub fn iconst_4(&mut self) {
        self.stack.push_const4();
    }

    pub fn iconst_5(&mut self) {
        self.stack.push_const5();
    }

    pub fn sipush(&mut self) {
        let v = self.read_i2();
        self.stack.push_int(v);
    }

    pub fn bipush(&mut self) {
        let v = self.read_i1();
        self.stack.push_int(v);
    }

    pub fn ldc(&mut self) {
        let pos = self.read_u1();
        self.load_constant(pos);
    }

    pub fn ldc_w(&mut self) {
        let pos = self.read_u2();
        self.load_constant(pos);
    }

    pub fn ldc2_w(&mut self) {
        self.ldc_w()
    }

    pub fn iload(&mut self) {
        let pos = self.read_u1();
        let v = self.local.get_int(pos);
        self.stack.push_int(v);
    }

    pub fn lload(&mut self) {
        let pos = self.read_u1();
        let v = self.local.get_long(pos);
        self.stack.push_long(v);
    }

    pub fn fload(&mut self) {
        let pos = self.read_u1();
        let v = self.local.get_float(pos);
        self.stack.push_float(v);
    }

    pub fn dload(&mut self) {
        let pos = self.read_u1();
        let v = self.local.get_double(pos);
        self.stack.push_double(v);
    }

    pub fn aload(&mut self) {
        let pos = self.read_u1();
        let v = self.local.get_ref(pos);
        self.stack.push_ref(v);
    }

    pub fn iload_0(&mut self) {
        let v = self.local.get_int(0);
        self.stack.push_int(v);
    }

    pub fn lload_0(&mut self) {
        let v = self.local.get_long(0);
        self.stack.push_long(v);
    }

    pub fn fload_0(&mut self) {
        let v = self.local.get_float(0);
        self.stack.push_float(v);
    }

    pub fn dload_0(&mut self) {
        let v = self.local.get_double(0);
        self.stack.push_double(v);
    }

    pub fn aload_0(&mut self) {
        let v = self.local.get_ref(0);
        self.stack.push_ref(v);
    }

    pub fn iload_1(&mut self) {
        let v = self.local.get_int(1);
        self.stack.push_int(v);
    }

    pub fn lload_1(&mut self) {
        let v = self.local.get_long(1);
        self.stack.push_long(v);
    }

    pub fn fload_1(&mut self) {
        let v = self.local.get_float(1);
        self.stack.push_float(v);
    }

    pub fn dload_1(&mut self) {
        let v = self.local.get_double(1);
        self.stack.push_double(v);
    }

    pub fn aload_1(&mut self) {
        let v = self.local.get_ref(1);
        self.stack.push_ref(v);
    }

    pub fn iload_2(&mut self) {
        let v = self.local.get_int(2);
        self.stack.push_int(v);
    }

    pub fn lload_2(&mut self) {
        let v = self.local.get_long(2);
        self.stack.push_long(v);
    }

    pub fn fload_2(&mut self) {
        let v = self.local.get_float(2);
        self.stack.push_float(v);
    }

    pub fn dload_2(&mut self) {
        let v = self.local.get_double(2);
        self.stack.push_double(v);
    }

    pub fn aload_2(&mut self) {
        let v = self.local.get_ref(2);
        self.stack.push_ref(v);
    }

    pub fn iload_3(&mut self) {
        let v = self.local.get_int(3);
        self.stack.push_int(v);
    }

    pub fn lload_3(&mut self) {
        let v = self.local.get_long(3);
        self.stack.push_long(v);
    }

    pub fn fload_3(&mut self) {
        let v = self.local.get_float(3);
        self.stack.push_float(v);
    }

    pub fn dload_3(&mut self) {
        let v = self.local.get_double(3);
        self.stack.push_double(v);
    }

    pub fn aload_3(&mut self) {
        let v = self.local.get_ref(3);
        self.stack.push_ref(v);
    }

    pub fn iaload(&mut self) {
        let thread = self.thread.clone();
        let pos = self.stack.pop_int();
        let rf = self.stack.pop_ref();
        match &rf.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = ary.get_elm_at(pos as usize);
                    let v = v.deref();
                    match &v.v {
                        Oop::Int(v) => {
                            self.stack.push_int(*v);
                        }
                        _ => unreachable!(),
                    }
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn saload(&mut self) {
        self.iaload();
    }

    pub fn caload(&mut self) {
        self.iaload();
    }

    pub fn baload(&mut self) {
        self.iaload();
    }

    pub fn laload(&mut self) {
        let thread = self.thread.clone();
        let pos = self.stack.pop_int();
        let rf = self.stack.pop_ref();
        match &rf.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = ary.get_elm_at(pos as usize);
                    let v = v.deref();
                    match &v.v {
                        Oop::Long(v) => {
                            self.stack.push_long(*v);
                        }
                        _ => unreachable!(),
                    }
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn faload(&mut self) {
        let thread = self.thread.clone();
        let pos = self.stack.pop_int();
        let rf = self.stack.pop_ref();
        match &rf.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = ary.get_elm_at(pos as usize);
                    let v = v.deref();
                    match &v.v {
                        Oop::Float(v) => {
                            self.stack.push_float(*v);
                        }
                        _ => unreachable!(),
                    }
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn daload(&mut self) {
        let thread = self.thread.clone();
        let pos = self.stack.pop_int();
        let rf = self.stack.pop_ref();
        match &rf.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = ary.get_elm_at(pos as usize);
                    let v = v.deref();
                    match &v.v {
                        Oop::Double(v) => {
                            self.stack.push_double(*v);
                        }
                        _ => unreachable!(),
                    }
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn aaload(&mut self) {
        let thread = self.thread.clone();
        let pos = self.stack.pop_int();
        let rf = self.stack.pop_ref();
        match &rf.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = ary.get_elm_at(pos as usize);
                    self.stack.push_ref(v);
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn istore(&mut self) {
        let pos = self.read_u1();
        let v = self.stack.pop_int();
        self.local.set_int(pos, v);
    }

    pub fn lstore(&mut self) {
        let pos = self.read_u1();
        let v = self.stack.pop_long();
        self.local.set_long(pos, v);
    }

    pub fn fstore(&mut self) {
        let pos = self.read_u1();
        let v = self.stack.pop_float();
        self.local.set_float(pos, v);
    }

    pub fn dstore(&mut self) {
        let pos = self.read_u1();
        let v = self.stack.pop_double();
        self.local.set_double(pos, v);
    }

    pub fn astore(&mut self) {
        let pos = self.read_u1();
        let v = self.stack.pop_ref();
        self.local.set_ref(pos, v);
    }

    pub fn istore_0(&mut self) {
        let v = self.stack.pop_int();
        self.local.set_int(0, v);
    }

    pub fn istore_1(&mut self) {
        let v = self.stack.pop_int();
        self.local.set_int(1, v);
    }

    pub fn istore_2(&mut self) {
        let v = self.stack.pop_int();
        self.local.set_int(2, v);
    }

    pub fn istore_3(&mut self) {
        let v = self.stack.pop_int();
        self.local.set_int(3, v);
    }

    pub fn lstore_0(&mut self) {
        let v = self.stack.pop_long();
        self.local.set_long(0, v);
    }

    pub fn lstore_1(&mut self) {
        let v = self.stack.pop_long();
        self.local.set_long(1, v);
    }

    pub fn lstore_2(&mut self) {
        let v = self.stack.pop_long();
        self.local.set_long(2, v);
    }

    pub fn lstore_3(&mut self) {
        let v = self.stack.pop_long();
        self.local.set_long(3, v);
    }

    pub fn fstore_0(&mut self) {
        let v = self.stack.pop_float();
        self.local.set_float(0, v);
    }

    pub fn fstore_1(&mut self) {
        let v = self.stack.pop_float();
        self.local.set_float(1, v);
    }

    pub fn fstore_2(&mut self) {
        let v = self.stack.pop_float();
        self.local.set_float(2, v);
    }

    pub fn fstore_3(&mut self) {
        let v = self.stack.pop_float();
        self.local.set_float(3, v);
    }

    pub fn dstore_0(&mut self) {
        let v = self.stack.pop_double();
        self.local.set_double(0, v);
    }

    pub fn dstore_1(&mut self) {
        let v = self.stack.pop_double();
        self.local.set_double(1, v);
    }

    pub fn dstore_2(&mut self) {
        let v = self.stack.pop_double();
        self.local.set_double(2, v);
    }

    pub fn dstore_3(&mut self) {
        let v = self.stack.pop_double();
        self.local.set_double(3, v);
    }

    pub fn astore_0(&mut self) {
        let v = self.stack.pop_ref();
        self.local.set_ref(0, v);
    }

    pub fn astore_1(&mut self) {
        let v = self.stack.pop_ref();
        self.local.set_ref(1, v);
    }

    pub fn astore_2(&mut self) {
        let v = self.stack.pop_ref();
        self.local.set_ref(2, v);
    }

    pub fn astore_3(&mut self) {
        let v = self.stack.pop_ref();
        self.local.set_ref(3, v);
    }

    pub fn bastore(&mut self) {
        self.iastore();
    }

    pub fn castore(&mut self) {
        self.iastore();
    }

    pub fn sastore(&mut self) {
        self.iastore();
    }

    pub fn iastore(&mut self) {
        let thread = self.thread.clone();
        let v = self.stack.pop_int();
        let pos = self.stack.pop_int();
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match &mut rff.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = OopDesc::new_int(v);
                    ary.set_elm_at(pos as usize, v);
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn lastore(&mut self) {
        let thread = self.thread.clone();
        let v = self.stack.pop_long();
        let pos = self.stack.pop_int();
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match &mut rff.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = OopDesc::new_long(v);
                    ary.set_elm_at(pos as usize, v);
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn fastore(&mut self) {
        let thread = self.thread.clone();
        let v = self.stack.pop_float();
        let pos = self.stack.pop_int();
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match &mut rff.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = OopDesc::new_float(v);
                    ary.set_elm_at(pos as usize, v);
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn dastore(&mut self) {
        let thread = self.thread.clone();
        let v = self.stack.pop_double();
        let pos = self.stack.pop_int();
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match &mut rff.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = OopDesc::new_double(v);
                    ary.set_elm_at(pos as usize, v);
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn aastore(&mut self) {
        let thread = self.thread.clone();
        let pos = self.stack.pop_int();
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match &mut rff.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                if (pos < 0) || (pos as usize >= ary.get_length()) {
                    let msg = format!("length is {}, but index is {}", len, pos);
                    JavaThread::throw_ext_with_msg(
                        thread,
                        consts::J_ARRAY_INDEX_OUT_OF_BOUNDS,
                        false,
                        msg,
                    );
                    self.handle_exception();
                } else {
                    let v = self.stack.pop_ref();
                    ary.set_elm_at(pos as usize, v);
                }
            }
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn pop(&mut self) {
        self.stack.drop_top();
    }

    pub fn pop2(&mut self) {
        self.stack.drop_top();
        self.stack.drop_top();
    }

    pub fn dup(&mut self) {
        let v = self.stack.pop_ref();
        self.stack.push_ref(v.clone());
        self.stack.push_ref(v);
    }

    pub fn dup_x1(&mut self) {
        let v1 = self.stack.pop_ref();
        let v2 = self.stack.pop_ref();
        self.stack.push_ref(v1.clone());
        self.stack.push_ref(v2);
        self.stack.push_ref(v1);
    }

    pub fn dup_x2(&mut self) {
        let v1 = self.stack.pop_ref();
        let v2 = self.stack.pop_ref();
        let v3 = self.stack.pop_ref();
        self.stack.push_ref(v1.clone());
        self.stack.push_ref(v3);
        self.stack.push_ref(v2);
        self.stack.push_ref(v1);
    }

    pub fn dup2(&mut self) {
        let v1 = self.stack.pop_ref();
        let v2 = self.stack.pop_ref();
        self.stack.push_ref(v2.clone());
        self.stack.push_ref(v1.clone());
        self.stack.push_ref(v2);
        self.stack.push_ref(v1);
    }

    pub fn dup2_x1(&mut self) {
        let v1 = self.stack.pop_ref();
        let v2 = self.stack.pop_ref();
        let v3 = self.stack.pop_ref();
        self.stack.push_ref(v2.clone());
        self.stack.push_ref(v1.clone());
        self.stack.push_ref(v3);
        self.stack.push_ref(v2);
        self.stack.push_ref(v1);
    }

    pub fn dup2_x2(&mut self) {
        let v1 = self.stack.pop_ref();
        let v2 = self.stack.pop_ref();
        let v3 = self.stack.pop_ref();
        let v4 = self.stack.pop_ref();
        self.stack.push_ref(v2.clone());
        self.stack.push_ref(v1.clone());
        self.stack.push_ref(v4);
        self.stack.push_ref(v3);
        self.stack.push_ref(v2);
        self.stack.push_ref(v1);
    }

    pub fn swap(&mut self) {
        let v1 = self.stack.pop_ref();
        let v2 = self.stack.pop_ref();
        self.stack.push_ref(v1);
        self.stack.push_ref(v2);
    }

    pub fn iadd(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        self.stack.push_int(v1 + v2);
    }

    pub fn ladd(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        self.stack.push_long(v1 + v2);
    }

    pub fn fadd(&mut self) {
        let v2 = self.stack.pop_float();
        let v1 = self.stack.pop_float();
        self.stack.push_float(v1 + v2);
    }

    pub fn dadd(&mut self) {
        let v2 = self.stack.pop_double();
        let v1 = self.stack.pop_double();
        self.stack.push_double(v1 + v2);
    }

    pub fn isub(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        self.stack.push_int(v1 - v2);
    }

    pub fn lsub(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        self.stack.push_long(v1 - v2);
    }

    pub fn fsub(&mut self) {
        let v2 = self.stack.pop_float();
        let v1 = self.stack.pop_float();
        self.stack.push_float(v1 - v2);
    }

    pub fn dsub(&mut self) {
        let v2 = self.stack.pop_double();
        let v1 = self.stack.pop_double();
        self.stack.push_double(v1 - v2);
    }

    pub fn imul(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        self.stack.push_int(v1 * v2);
    }

    pub fn lmul(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        self.stack.push_long(v1 * v2);
    }

    pub fn fmul(&mut self) {
        let v2 = self.stack.pop_float();
        let v1 = self.stack.pop_float();
        self.stack.push_float(v1 * v2);
    }

    pub fn dmul(&mut self) {
        let v2 = self.stack.pop_double();
        let v1 = self.stack.pop_double();
        self.stack.push_double(v1 * v2);
    }

    pub fn idiv(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v2 == 0 {
            let thread = self.thread.clone();
            JavaThread::throw_ext_with_msg2(
                thread,
                consts::J_ARITHMETIC_EX,
                false,
                b"divide by zero",
            );
            self.handle_exception();
        } else {
            self.stack.push_int(v1 / v2);
        }
    }

    pub fn ldiv(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        if v2 == 0 {
            let thread = self.thread.clone();
            JavaThread::throw_ext_with_msg2(
                thread,
                consts::J_ARITHMETIC_EX,
                false,
                b"divide by zero",
            );
            self.handle_exception();
        } else {
            self.stack.push_long(v1 / v2);
        }
    }

    pub fn fdiv(&mut self) {
        let v2 = self.stack.pop_float();
        let v1 = self.stack.pop_float();
        if v2 == 0.0 {
            let thread = self.thread.clone();
            JavaThread::throw_ext_with_msg2(
                thread,
                consts::J_ARITHMETIC_EX,
                false,
                b"divide by zero",
            );
            self.handle_exception();
        } else {
            self.stack.push_float(v1 / v2);
        }
    }

    pub fn ddiv(&mut self) {
        let v2 = self.stack.pop_double();
        let v1 = self.stack.pop_double();
        if v2 == 0.0 {
            let thread = self.thread.clone();
            JavaThread::throw_ext_with_msg2(
                thread,
                consts::J_ARITHMETIC_EX,
                false,
                b"divide by zero",
            );
            self.handle_exception();
        } else {
            self.stack.push_double(v1 / v2);
        }
    }

    pub fn irem(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v2 == 0 {
            let thread = self.thread.clone();
            JavaThread::throw_ext_with_msg2(
                thread,
                consts::J_ARITHMETIC_EX,
                false,
                b"divide by zero",
            );
            self.handle_exception();
        } else {
            self.stack.push_int(v1 - (v1 / v2) * v2);
        }
    }

    pub fn lrem(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        if v2 == 0 {
            let thread = self.thread.clone();
            JavaThread::throw_ext_with_msg2(
                thread,
                consts::J_ARITHMETIC_EX,
                false,
                b"divide by zero",
            );
            self.handle_exception();
        } else {
            self.stack.push_long(v1 - (v1 / v2) * v2);
        }
    }

    pub fn frem(&mut self) {
        panic!("Use of deprecated instruction frem, please check your Java compiler");
    }

    pub fn drem(&mut self) {
        panic!("Use of deprecated instruction drem, please check your Java compiler");
    }

    pub fn ineg(&mut self) {
        let v = self.stack.pop_int();
        self.stack.push_int(-v);
    }

    pub fn lneg(&mut self) {
        let v = self.stack.pop_long();
        self.stack.push_long(-v);
    }

    pub fn fneg(&mut self) {
        panic!("Use of deprecated instruction fneg, please check your Java compiler");
    }

    pub fn dneg(&mut self) {
        panic!("Use of deprecated instruction dneg, please check your Java compiler");
    }

    pub fn ishl(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        let s = v2 & 0x1F;
        self.stack.push_int(v1 << s);
    }

    pub fn lshl(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_long();
        let s = (v2 & 0x3F) as i64;
        self.stack.push_long(v1 << s);
    }

    pub fn ishr(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        let s = v2 & 0x1F;
        self.stack.push_int(v1 >> s);
    }

    pub fn lshr(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_long();
        let s = (v2 & 0x3F) as i64;
        self.stack.push_long(v1 >> s);
    }

    pub fn iushr(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        let s = v2 & 0x1F;
        if v1 >= 0 {
            self.stack.push_int(v1 >> s);
        } else {
            self.stack.push_int((v1 >> s) + (2 << !s));
        }
    }

    pub fn lushr(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_long();
        let s = (v2 & 0x3F) as i64;
        if v1 >= 0 {
            self.stack.push_long(v1 >> s);
        } else {
            self.stack.push_long((v1 >> s) + (2 << !s));
        }
    }

    pub fn iand(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        self.stack.push_int(v1 & v2);
    }

    pub fn land(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        self.stack.push_long(v1 & v2);
    }

    pub fn ior(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        self.stack.push_int(v1 | v2);
    }

    pub fn lor(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        self.stack.push_long(v1 | v2);
    }

    pub fn ixor(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        self.stack.push_int(v1 ^ v2);
    }

    pub fn lxor(&mut self) {
        let v2 = self.stack.pop_long();
        let v1 = self.stack.pop_long();
        self.stack.push_long(v1 ^ v2);
    }

    pub fn iinc(&mut self) {
        let pos = self.read_u1();
        let factor = self.read_i1();

        let v = self.local.get_int(pos);
        let v = v + factor;
        self.local.set_int(pos, v);
    }

    pub fn i2l(&mut self) {
        let v = self.stack.pop_int();
        self.stack.push_long(v as i64);
    }

    pub fn i2f(&mut self) {
        let v = self.stack.pop_int();
        self.stack.push_float(v as f32);
    }

    pub fn i2d(&mut self) {
        let v = self.stack.pop_int();
        self.stack.push_double(v as f64);
    }

    pub fn l2i(&mut self) {
        let v = self.stack.pop_long();
        self.stack.push_int(v as i32);
    }

    pub fn l2f(&mut self) {
        let v = self.stack.pop_long();
        self.stack.push_float(v as f32);
    }

    pub fn l2d(&mut self) {
        let v = self.stack.pop_long();
        self.stack.push_double(v as f64);
    }

    pub fn f2i(&mut self) {
        let v = self.stack.pop_float();
        if v.is_nan() {
            self.stack.push_int(0);
        } else if v.is_infinite() {
            if v.is_sign_positive() {
                self.stack.push_int(std::i32::MAX);
            } else {
                self.stack.push_int(std::i32::MIN);
            }
        } else {
            self.stack.push_int(v as i32);
        }
    }

    pub fn f2l(&mut self) {
        let v = self.stack.pop_float();
        if v.is_nan() {
            self.stack.push_long(0);
        } else if v.is_infinite() {
            if v.is_sign_positive() {
                self.stack.push_long(std::i64::MAX);
            } else {
                self.stack.push_long(std::i64::MIN);
            }
        } else {
            self.stack.push_long(v as i64);
        }
    }

    pub fn f2d(&mut self) {
        let v = self.stack.pop_float();
        self.stack.push_double(v as f64);
    }

    pub fn d2i(&mut self) {
        let v = self.stack.pop_double();
        if v.is_nan() {
            self.stack.push_int(0);
        } else if v.is_infinite() {
            if v.is_sign_positive() {
                self.stack.push_int(std::i32::MAX);
            } else {
                self.stack.push_int(std::i32::MIN);
            }
        } else {
            self.stack.push_int(v as i32);
        }
    }

    pub fn d2l(&mut self) {
        let v = self.stack.pop_double();
        if v.is_nan() {
            self.stack.push_long(0);
        } else if v.is_infinite() {
            if v.is_sign_positive() {
                self.stack.push_long(std::i64::MAX);
            } else {
                self.stack.push_long(std::i64::MIN);
            }
        } else {
            self.stack.push_long(v as i64);
        }
    }

    pub fn d2f(&mut self) {
        let v = self.stack.pop_double();
        self.stack.push_float(v as f32);
    }

    pub fn i2b(&mut self) {
        let v = self.stack.pop_int();
        let v = v as i8;
        self.stack.push_int(v as i32);
    }

    pub fn i2c(&mut self) {
        let v = self.stack.pop_int();
        let v = v as u16;
        self.stack.push_int(v as i32);
    }

    pub fn i2s(&mut self) {
        let v = self.stack.pop_int();
        let v = v as i16;
        self.stack.push_int(v as i32);
    }

    pub fn lcmp(&mut self) {
        let v1 = self.stack.pop_long();
        let v2 = self.stack.pop_long();
        if v1 > v2 {
            self.stack.push_int(-1);
        } else if v1 < v2 {
            self.stack.push_int(1);
        } else {
            self.stack.push_int(0);
        }
    }

    pub fn fcmpl(&mut self) {
        let v1 = self.stack.pop_float();
        let v2 = self.stack.pop_float();
        if v1.is_nan() || v2.is_nan() {
            self.stack.push_int(-1);
        } else if v1 > v2 {
            self.stack.push_int(-1);
        } else if v1 < v2 {
            self.stack.push_int(1);
        } else {
            self.stack.push_int(0);
        }
    }

    pub fn fcmpg(&mut self) {
        let v1 = self.stack.pop_float();
        let v2 = self.stack.pop_float();
        if v1.is_nan() || v2.is_nan() {
            self.stack.push_int(1);
        } else if v1 > v2 {
            self.stack.push_int(-1);
        } else if v1 < v2 {
            self.stack.push_int(1);
        } else {
            self.stack.push_int(0);
        }
    }

    pub fn dcmpl(&mut self) {
        let v1 = self.stack.pop_double();
        let v2 = self.stack.pop_double();
        if v1.is_nan() || v2.is_nan() {
            self.stack.push_int(-1);
        } else if v1 > v2 {
            self.stack.push_int(-1);
        } else if v1 < v2 {
            self.stack.push_int(1);
        } else {
            self.stack.push_int(0);
        }
    }

    pub fn dcmpg(&mut self) {
        let v1 = self.stack.pop_double();
        let v2 = self.stack.pop_double();
        if v1.is_nan() || v2.is_nan() {
            self.stack.push_int(1);
        } else if v1 > v2 {
            self.stack.push_int(-1);
        } else if v1 < v2 {
            self.stack.push_int(1);
        } else {
            self.stack.push_int(0);
        }
    }

    pub fn ifeq(&mut self) {
        let v = self.stack.pop_int();
        if v == 0 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn ifne(&mut self) {
        let v = self.stack.pop_int();
        if v != 0 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn iflt(&mut self) {
        let v = self.stack.pop_int();
        if v < 0 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn ifge(&mut self) {
        let v = self.stack.pop_int();
        if v >= 0 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn ifgt(&mut self) {
        let v = self.stack.pop_int();
        if v > 0 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn ifle(&mut self) {
        let v = self.stack.pop_int();
        if v <= 0 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_icmpeq(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v1 == v2 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_icmpne(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v1 != v2 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_icmplt(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v1 < v2 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_icmpge(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v1 >= v2 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_icmpgt(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v1 > v2 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_icmple(&mut self) {
        let v2 = self.stack.pop_int();
        let v1 = self.stack.pop_int();
        if v1 <= v2 {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_acmpeq(&mut self) {
        let v2 = self.stack.pop_ref();
        let v1 = self.stack.pop_ref();
        if Arc::ptr_eq(&v1, &v2) {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn if_acmpne(&mut self) {
        let v2 = self.stack.pop_ref();
        let v1 = self.stack.pop_ref();
        if !Arc::ptr_eq(&v1, &v2) {
            let branch = self.read_i2();
            self.pc += branch;
            self.pc += -1;
        } else {
            self.pc += 2;
        }
    }

    pub fn goto(&mut self) {
        let branch = self.read_i2();
        self.pc += branch;
        self.pc += -1;
    }

    pub fn jsr(&mut self) {
        self.pc += 2;
        panic!("Use of deprecated instruction jsr, please check your Java compiler");
    }

    pub fn ret(&mut self) {
        self.pc += 1;
        panic!("Use of deprecated instruction ret, please check your Java compiler");
    }

    pub fn table_switch(&mut self) {
        //todo: impl
    }

    pub fn lookup_switch(&mut self) {
        //todo: impl
    }

    pub fn ireturn(&mut self) {
        let v = self.stack.pop_int();
        let v = OopDesc::new_int(v);
        self.set_return(v);
    }

    pub fn lreturn(&mut self) {
        let v = self.stack.pop_long();
        let v = OopDesc::new_long(v);
        self.set_return(v);
    }

    pub fn freturn(&mut self) {
        let v = self.stack.pop_float();
        let v = OopDesc::new_float(v);
        self.set_return(v);
    }

    pub fn dreturn(&mut self) {
        let v = self.stack.pop_double();
        let v = OopDesc::new_double(v);
        self.set_return(v);
    }

    pub fn areturn(&mut self) {
        let v = self.stack.pop_ref();
        self.set_return(v);
    }

    pub fn return_(&mut self) {
        self.set_return(oop_consts::get_null());
    }

    pub fn get_static(&mut self) {
        let cp_idx = self.read_i2();
        let (v, value_type) = self.get_field_helper(oop_consts::get_null(), cp_idx);
        if self.thread.is_exception_occurred() {
            self.handle_exception();
        }

        match value_type {
            ValueType::OBJECT | ValueType::ARRAY => self.stack.push_ref(v),
            ValueType::INT
            | ValueType::SHORT
            | ValueType::CHAR
            | ValueType::BOOLEAN
            | ValueType::BYTE => match v.v {
                Oop::Int(v) => self.stack.push_int(v),
                _ => unreachable!(),
            },
            ValueType::FLOAT => match v.v {
                Oop::Float(v) => self.stack.push_float(v),
                _ => unreachable!(),
            },
            ValueType::DOUBLE => match v.v {
                Oop::Double(v) => self.stack.push_double(v),
                _ => unreachable!(),
            },
            ValueType::LONG => match v.v {
                Oop::Long(v) => self.stack.push_long(v),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    pub fn put_static(&mut self) {
        let cp_idx = self.read_i2();
        self.put_field_helper(cp_idx, true);
        if self.thread.is_exception_occurred() {
            self.handle_exception();
        }
    }

    pub fn get_field(&mut self) {
        let cp_idx = self.read_i2();
        let rf = self.stack.pop_ref();
        if Arc::ptr_eq(&rf, &oop_consts::get_null()) {
            let thread = self.thread.clone();
            JavaThread::throw_ext(thread, consts::J_NPE, false);
            self.handle_exception();
        } else {
            self.get_field_helper(rf, cp_idx);
            if self.thread.is_exception_occurred() {
                self.handle_exception();
            }
        }
    }

    pub fn put_field(&mut self) {
        let cp_idx = self.read_i2();
        self.put_field_helper(cp_idx, false);
    }

    pub fn invoke_virtual(&mut self) {
        //todo: impl
    }

    pub fn invoke_special(&mut self) {
        //todo: impl
    }

    pub fn invoke_static(&mut self) {
        //todo: impl
    }

    pub fn invoke_interface(&mut self) {
        //todo: impl
    }

    pub fn invoke_dynamic(&mut self) {
        //todo: impl
    }

    pub fn new_(&mut self) {
        let class = {
            let constant_idx = self.read_i2();
            let class = self.class.lock().unwrap();
            let cp = &class.class_file.cp;
            match runtime::require_class2(constant_idx as u16, cp) {
                Some(class) => {
                    {
                        let mut class = class.lock().unwrap();
                        if class.typ != oop::ClassType::InstanceClass {
                            unreachable!()
                        }

                        class.init_class(self.thread.clone());
                    }

                    class
                }
                None => panic!("Cannot get class info from constant pool"),
            }
        };

        if self.thread.is_exception_occurred() {
            self.handle_exception();
        } else {
            let v = oop::InstOopDesc::new(class);
            let v = oop::OopDesc::new_inst(v);
            self.stack.push_ref(v);
        }
    }

    pub fn new_array(&mut self) {
        //todo: impl
    }

    pub fn anew_array(&mut self) {
        //todo: impl
    }

    pub fn array_length(&mut self) {
        let rf = self.stack.pop_ref();
        match &rf.v {
            Oop::Array(ary) => {
                let len = ary.get_length();
                self.stack.push_int(len as i32);
            }
            Oop::Null => {
                let thread = self.thread.clone();
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => unreachable!(),
        }
    }

    pub fn athrow(&mut self) {
        let thread = self.thread.clone();
        let rf = self.stack.pop_ref();
        match rf.v {
            Oop::Null => {
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => {
                let handler = JavaThread::try_handle_exception(thread, rf.clone());
                if handler > 0 {
                    trace!("athrow: exception handler found at offset: {}", handler);
                    self.stack.clear();
                    self.stack.push_ref(rf);
                    self.goto_abs(handler);
                } else {
                    trace!("athrow: exception handler not found, rethrowing it to caller");
                    self.set_return(rf);
                }
            }
        }
    }

    pub fn check_cast(&mut self) {
        //todo: impl
    }

    pub fn instance_of(&mut self) {
        //todo: impl
    }

    pub fn monitor_enter(&mut self) {
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match rff.v {
            Oop::Null => {
                let thread = self.thread.clone();
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => {
                rff.monitor_enter();
            }
        }
    }

    pub fn monitor_exit(&mut self) {
        let mut rf = self.stack.pop_ref();
        let rff = Arc::get_mut(&mut rf).unwrap();
        match rff.v {
            Oop::Null => {
                let thread = self.thread.clone();
                JavaThread::throw_ext(thread, consts::J_NPE, false);
                self.handle_exception();
            }
            _ => {
                rff.monitor_exit();
            }
        }
    }

    pub fn wide(&mut self) {
        panic!("Use of deprecated instruction wide, please check your Java compiler")
    }

    pub fn multi_anew_array(&mut self) {
        //todo: impl
    }

    pub fn if_null(&mut self) {
        let v = self.stack.pop_ref();
        let v = v.deref();
        match v.v {
            Oop::Null => {
                let branch = self.read_i2();
                self.pc += branch;
                self.pc += -1;
            }
            _ => self.pc += 2,
        }
    }

    pub fn if_non_null(&mut self) {
        let v = self.stack.pop_ref();
        let v = v.deref();
        match v.v {
            Oop::Null => self.pc += 2,
            _ => {
                let branch = self.read_i2();
                self.pc += branch;
                self.pc += -1;
            }
        }
    }

    pub fn goto_w(&mut self) {
        self.pc += 4;
        panic!("Use of deprecated instruction goto_w, please check your Java compiler")
    }

    pub fn jsr_w(&mut self) {
        self.pc += 4;
        panic!("Use of deprecated instruction jsr_w, please check your Java compiler")
    }

    pub fn other_wise(&mut self) {
        let pc = self.pc - 1;
        panic!(
            "Use of undefined bytecode: {} at {}",
            self.code[pc as usize], pc
        );
    }
}
