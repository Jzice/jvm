use super::FieldTranslator;
use super::{MethodTranslation, MethodTranslator};
use crate::trans::AccessFlagsTranslator;
use crate::trans::{AccessFlagHelper, FieldTranslation};
use class_parser::ClassSignature;
use classfile::AttributeType;
use classfile::ClassFile;
use classfile::{constant_pool, SignatureType};

const S_UNKNOWN: &str = "unknown";

pub struct Translator<'a> {
    cf: &'a ClassFile,
}

impl<'a> Translator<'a> {
    pub fn new(cf: &'a ClassFile) -> Self {
        Self { cf }
    }
}

impl<'a> Translator<'a> {
    pub fn source_file(&self) -> String {
        for it in &self.cf.attrs {
            match it {
                AttributeType::SourceFile { source_file_index } => {
                    return constant_pool::get_utf8(&self.cf.cp, *source_file_index as usize)
                        .map_or_else(
                            || S_UNKNOWN.into(),
                            |v| String::from_utf8_lossy(v.as_slice()).into(),
                        );
                }
                _ => (),
            }
        }

        String::from(S_UNKNOWN)
    }

    pub fn this_class(&self) -> String {
        constant_pool::get_class_name(&self.cf.cp, self.cf.this_class as usize).map_or_else(
            || S_UNKNOWN.into(),
            |v| String::from_utf8_lossy(v.as_slice()).replace("/", "."),
        )
    }

    pub fn super_class(&self) -> String {
        constant_pool::get_class_name(&self.cf.cp, self.cf.super_class as usize).map_or_else(
            || S_UNKNOWN.into(),
            |v| String::from_utf8_lossy(v.as_slice()).replace("/", "."),
        )
    }

    pub fn parent_interfaces(&self) -> Vec<String> {
        assert_ne!(self.cf.interfaces.len(), 0);
        let mut interfaces = Vec::with_capacity(self.cf.interfaces.len());

        for it in self.cf.interfaces.iter() {
            let name = constant_pool::get_class_name(&self.cf.cp, *it as usize).map_or_else(
                || S_UNKNOWN.into(),
                |v| String::from_utf8_lossy(v.as_slice()).replace("/", "."),
            );
            interfaces.push(name);
        }

        interfaces
    }

    pub fn access_flags(&self) -> String {
        let flags = self.cf.acc_flags;
        let t = AccessFlagsTranslator::new(flags);
        t.class_access_flags(false)
    }

    pub fn access_flags_name(&self) -> String {
        let flags = self.cf.acc_flags;
        let t = AccessFlagsTranslator::new(flags);
        t.access_flag_inner()
    }

    pub fn signature_raw(&self) -> Option<String> {
        self.cf.signature().map(|idx| {
            let v = constant_pool::get_utf8(&self.cf.cp, idx).unwrap();
            let signature = String::from_utf8_lossy(v.as_slice());
            format!("Signature: #{:<28} // {}", idx, signature)
        })
    }

    pub fn signature(&self) -> Option<Vec<SignatureType>> {
        self.cf.signature().map(|idx| {
            let v = constant_pool::get_utf8(&self.cf.cp, idx).unwrap();
            let v = ClassSignature::new(v.as_slice());
            v.items.clone()
        })
    }

    /*
    desc = public HelloWorld();, acc_flags = 1
    desc = public static void main(java.lang.String[]);, acc_flags = 9
    desc = private void private_method();, acc_flags = 2
    desc = protected void protected_method();, acc_flags = 4
    desc = void package_method();, acc_flags = 0
    desc = public void public_method();, acc_flags = 1
    */
    pub fn methods(
        &self,
        with_line_num: bool,
        with_code: bool,
        flags: u16,
    ) -> Vec<MethodTranslation> {
        let mut methods = Vec::with_capacity(self.cf.methods.len());
        for it in self.cf.methods.iter() {
            if it.acc_flags.is_bridge() {
                continue;
            }

            if flags.compare(it.acc_flags) > 0 {
                continue;
            }

            let t = MethodTranslator::new(self.cf, it);
            methods.push(t.get(with_line_num, with_code));
        }

        methods
    }

    pub fn fields(&self, flags: u16) -> Vec<FieldTranslation> {
        let mut fields = Vec::with_capacity(self.cf.fields.len());
        for it in self.cf.fields.iter() {
            let t = FieldTranslator::new(self.cf, it);

            if it.acc_flags.is_synthetic() {
                continue;
            }

            if flags.compare(it.acc_flags) > 0 {
                continue;
            }

            fields.push(t.get());
        }

        fields
    }

    pub fn inner_classes(&self) -> Vec<String> {
        let mut r = vec![];
        match self.cf.inner_classes() {
            Some(inners) => {
                for it in inners.iter() {
                    let inner_class_info_index = it.inner_class_info_index;
                    let outer_class_info_index = it.outer_class_info_index;
                    let inner_name_index = it.inner_name_index;
                    let inner_class_access_flags = it.inner_class_access_flags;
                    let flags = AccessFlagsTranslator::new(inner_class_access_flags);

                    //top-level class or interface
                    if outer_class_info_index == 0 {
                        let inner_class_info = constant_pool::get_class_name(
                            &self.cf.cp,
                            inner_class_info_index as usize,
                        )
                        .unwrap();
                        let v = format!(
                            "#{}; //class {}",
                            inner_class_info_index,
                            String::from_utf8_lossy(inner_class_info.as_slice())
                        );
                        r.push(v);
                    } else {
                        if inner_class_access_flags.is_public() {
                            let inner_class_info = constant_pool::get_class_name(
                                &self.cf.cp,
                                inner_class_info_index as usize,
                            )
                            .unwrap();
                            let inner_class_info =
                                String::from_utf8_lossy(inner_class_info.as_slice());
                            let inner_name =
                                constant_pool::get_utf8(&self.cf.cp, inner_name_index as usize)
                                    .unwrap();
                            let inner_name = String::from_utf8_lossy(inner_name.as_slice());
                            let outer_class_info = constant_pool::get_class_name(
                                &self.cf.cp,
                                outer_class_info_index as usize,
                            )
                            .unwrap();
                            let outer_class_info =
                                String::from_utf8_lossy(outer_class_info.as_slice());
                            let flags = flags.class_access_flags(true);
                            let v = format!(
                                "{} #{}= #{} of #{}; //{}=class {} of class {}",
                                flags,
                                inner_name_index,
                                inner_class_info_index,
                                outer_class_info_index,
                                inner_name,
                                inner_class_info,
                                outer_class_info
                            );
                            r.push(v);
                        }
                    }
                }
            }
            None => (),
        }
        r
    }
}
