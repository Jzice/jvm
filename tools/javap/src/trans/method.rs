use crate::trans::AccessFlagHelper;
use crate::trans::SignatureTypeTranslator;
use classfile::{constant_pool, ClassFile, MethodInfo, MethodSignature};

pub struct Translator<'a> {
    cf: &'a ClassFile,
    method: &'a MethodInfo,
}

impl<'a> Translator<'a> {
    pub fn new(cf: &'a ClassFile, method: &'a MethodInfo) -> Self {
        Self { cf, method }
    }
}

impl<'a> Translator<'a> {
    pub fn get(&self) -> String {
        vec![self.access_flags(), self.return_type(), {
            //name
            let mut r = self.name();

            //args
            r.push_str("(");
            let args = self.args();
            r.push_str(args.join(", ").as_str());
            r.push_str(")");

            r.push_str(";");

            r
        }]
        .join(" ")
    }
}

impl<'a> Translator<'a> {
    fn access_flags(&self) -> String {
        let flags = self.method.acc_flags;

        let mut name = String::new();

        if flags.is_public() {
            name.push_str("public");
        } else if flags.is_protected() {
            name.push_str("protected");
        } else if flags.is_private() {
            name.push_str("private");
        }

        if flags.is_final() {
            name.push_str(" final");
        } else if flags.is_abstract() {
            name.push_str(" abstract");
        }

        name
    }

    fn return_type(&self) -> String {
        let desc = constant_pool::get_utf8(&self.cf.cp, self.method.desc_index as usize).unwrap();
        let signature = MethodSignature::new(desc.as_slice());

        signature.retype.into_string()
    }

    fn name(&self) -> String {
        let name = constant_pool::get_utf8(&self.cf.cp, self.method.name_index as usize).unwrap();

        String::from_utf8_lossy(name.as_slice()).to_string()
    }

    fn args(&self) -> Vec<String> {
        let desc = constant_pool::get_utf8(&self.cf.cp, self.method.desc_index as usize).unwrap();
        let signature = MethodSignature::new(desc.as_slice());
        signature.args.iter().map(|it| it.into_string()).collect()
    }
}