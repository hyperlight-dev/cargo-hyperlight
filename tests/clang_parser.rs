//! Tests for the logic used in the clang frontend wrapper

#[path = "../src/wrapper/_clang_parser.rs"]
mod clang_parser;

use std::ffi::OsStr;

use clang_parser::parse_clang_cmd;

macro_rules! parse_test {
    ($name:ident, $args:tt, $non_flag:tt, $non_link_flag:tt, $will_link:tt) => {
        #[test]
        fn $name() {
            let args: &[&str] = &$args;
            let strs = args.iter().map(|x| OsStr::new(x));
            let info = parse_clang_cmd(strs);
            assert_eq!(info.has_non_flag, $non_flag);
            assert_eq!(info.has_non_link_flag, $non_link_flag);
            assert_eq!(info.will_link(), $will_link);
        }
    };
}

parse_test!(empty, [], false, false, false);
parse_test!(no_args, ["-v"], false, false, false);
parse_test!(linking, ["foo.c"], true, false, true);
parse_test!(non_link_flag_c, ["-c", "foo.c"], true, true, false);
parse_test!(non_link_flag_s, ["-S", "foo.c"], true, true, false);
parse_test!(non_link_flag_e, ["-E", "foo.c"], true, true, false);
parse_test!(non_link_flag_m, ["-M", "foo.c"], true, true, false);
parse_test!(non_link_flag_mm, ["-MM", "foo.c"], true, true, false);
parse_test!(
    non_link_flag_lang_space,
    ["-x", "foo-header", "foo.c"],
    true,
    true,
    false
);
parse_test!(
    non_link_flag_lang_nospace,
    ["-xfoo-header", "foo.c"],
    true,
    true,
    false
);
parse_test!(end_of_flags_empty, ["--"], false, false, false);
parse_test!(end_of_flags_with_flag_like, ["--", "-v"], true, false, true);
parse_test!(
    end_of_flags_with_non_link_flag_like,
    ["--", "-c"],
    true,
    false,
    true
);
parse_test!(
    end_of_flags_with_non_link_flag,
    ["-c", "--", "-v"],
    true,
    true,
    false
);
