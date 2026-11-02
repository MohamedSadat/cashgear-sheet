//! Excel's number formatting, as a spec. Every expected text here is what Excel itself shows on an
//! en-US system, read from `excel-text.jsonl` or from probes made while writing this crate.

// The 15-digit value below is a test input, not an attempt at pi.
#![allow(clippy::approx_constant)]

use cgs_numfmt::{
    edit_text, serial_to_datetime, text_function, DateTimeParts, Display, FormatColor, Locale,
    NumFmt, Options, Value,
};

fn with(format: &NumFmt, value: Value<'_>) -> (String, Display) {
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: false,
    };
    let mut out = String::new();
    let display = format.format(value, &options, &mut out);
    (out, display)
}

fn number(code: &str, value: f64) -> String {
    let format = NumFmt::parse(code);
    assert!(format.is_valid(), "{code} should be valid");
    let (out, display) = with(&format, Value::Number(value));
    if display.hashes {
        return "####".to_string();
    }
    out
}

fn text(code: &str, value: &str) -> String {
    with(&NumFmt::parse(code), Value::Text(value)).0
}

fn date1904(code: &str, value: f64) -> String {
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: true,
    };
    let mut out = String::new();
    let display = NumFmt::parse(code).format(Value::Number(value), &options, &mut out);
    if display.hashes {
        return "####".to_string();
    }
    out
}

#[test]
fn general_fits_eleven_characters() {
    assert_eq!(number("General", 12345678901.0), "12345678901");
    assert_eq!(number("General", 123456789012.0), "1.23457E+11");
    assert_eq!(number("General", 1e15), "1E+15");
    assert_eq!(number("General", 1234567890123456.0), "1.23457E+15");
    assert_eq!(number("General", 1234.5678), "1234.5678");
    assert_eq!(number("General", 3.14159265358979), "3.141592654");
    assert_eq!(number("General", 0.333333333333333), "0.333333333");
    assert_eq!(number("General", 0.00012345678), "0.000123457");
    assert_eq!(number("General", 1.23456789e-5), "1.23457E-05");
    assert_eq!(number("General", 0.00001), "0.00001");
    assert_eq!(number("General", 0.00001234), "0.00001234");
    assert_eq!(number("General", 1e-9), "0.000000001");
    assert_eq!(number("General", 1e-10), "1E-10");
}

#[test]
fn general_gives_up_a_digit_to_a_long_exponent() {
    assert_eq!(number("General", 1.23456e98), "1.23456E+98");
    assert_eq!(number("General", 1.23456e99), "1.2346E+99");
    assert_eq!(number("General", 1.23456e-99), "1.2346E-99");
    assert_eq!(number("General", 1.7976931348623157e308), "1.7977E+308");
    assert_eq!(number("General", 1e100), "1E+100");
}

#[test]
fn general_rounds_in_decimal_and_carries() {
    assert_eq!(number("General", 45000.999999), "45001");
    assert_eq!(number("General", 0.1 + 0.2), "0.3");
    assert_eq!(number("General", 99999999999.5), "1E+11");
    assert_eq!(number("General", -12345678901.0), "-12345678901");
    assert_eq!(number("General", -123456789012.0), "-1.23457E+11");
}

#[test]
fn general_tells_the_worker_it_may_shorten() {
    let (_, display) = with(&NumFmt::general(), Value::Number(1.5));
    assert!(display.general);
    let (_, display) = with(&NumFmt::parse("0.00"), Value::Number(1.5));
    assert!(!display.general);
}

#[test]
fn placeholders_show_digits_zeros_spaces_or_nothing() {
    assert_eq!(number("0", 5.0), "5");
    assert_eq!(number("000", 5.0), "005");
    assert_eq!(number("#", 0.0), "");
    assert_eq!(number("?", 0.0), " ");
    assert_eq!(number("#0#", 1.0), "01");
    assert_eq!(number("?0?", 1.0), " 01");
    assert_eq!(number("0?#", 1.0), "0 1");
    assert_eq!(number("0.##", 1.0), "1.");
    assert_eq!(number("0.##", 0.5), "0.5");
    assert_eq!(number("0.0?", 1.0), "1.0 ");
    assert_eq!(number("0.#?0", 1.0), "1. 0");
    assert_eq!(number("0.#?0", 0.5), "0.5 0");
    assert_eq!(number("??.??", 1.0), " 1.  ");
    assert_eq!(number(".0", 0.5), ".5");
    assert_eq!(number(".0", 1234.5678), "1234.6");
    assert_eq!(number("#.", 0.0), ".");
}

#[test]
fn extra_digits_come_out_at_the_first_placeholder() {
    assert_eq!(number("0 0", 1234.0), "123 4");
    assert_eq!(number("000-00-0000", 1234567.0), "001-23-4567");
    assert_eq!(number("0\"x\"0\"y\"0", 12345.0), "123x4y5");
    assert_eq!(number("#\"x\"#", 1.0), "x1");
    assert_eq!(number("0.0\"x\"0", 1234.5678), "1234.5x7");
}

#[test]
fn a_comma_between_placeholders_groups_thousands() {
    assert_eq!(number("#,##0", 1234567.0), "1,234,567");
    assert_eq!(number("#,##0", 5.0), "5");
    assert_eq!(number("0,000", 5.0), "0,005");
    assert_eq!(number("0,0", 1234.0), "1,234");
    assert_eq!(number("0,0", 5.0), "05");
    assert_eq!(number("#,#", 1234.0), "1,234");
    assert_eq!(number("?,???", 1.0), "    1");
    assert_eq!(number("?,???", 1234.0), "1,234");
}

#[test]
fn a_comma_after_the_last_placeholder_scales_by_a_thousand() {
    assert_eq!(number("0,", 1234.0), "1");
    assert_eq!(number("0,", 1234567.0), "1235");
    assert_eq!(number("0,,", 1234567.0), "1");
    assert_eq!(number("#,##0,", 1234567.0), "1,235");
    assert_eq!(number("0.0,", 1234.0), "1.2");
    assert_eq!(number("0,.0", 1234.0), "1.2");
    assert_eq!(number("#,##0.00,", 1234567.0), "1,234.57");
    assert_eq!(number("#,", 5.0), "");
    assert_eq!(number("0,%", 1234.0), "123%");
}

#[test]
fn a_comma_anywhere_else_is_itself_or_nothing() {
    assert_eq!(number(",0", 5.0), ",5");
    assert_eq!(number("0%,", 1.0), "100%,");
    assert_eq!(number("0.0,0", 1.0), "1.00");
    assert_eq!(number("0.,0", 1.0), "1.0");
}

#[test]
fn a_percent_sign_multiplies_by_a_hundred_each_time() {
    assert_eq!(number("0%", 0.5), "50%");
    assert_eq!(number("0.0%", 0.125), "12.5%");
    assert_eq!(number("0%%", 1.0), "10000%%");
    assert_eq!(number("%0", 1.0), "%100");
    assert_eq!(number("0%0", 1.0), "10%0");
    assert_eq!(number("%", 5.0), "%");
}

#[test]
fn rounding_works_on_the_displayed_digits_not_the_binary_value() {
    // 1.005 is just under in binary, but 15 significant digits make it exactly 1.005.
    assert_eq!(number("0.00", 1.005), "1.01");
    assert_eq!(number("0.00", 2.675), "2.68");
    assert_eq!(number("0.00", 1.00499999999999), "1.00");
    assert_eq!(number("0.00", 1.004999999999999), "1.01");
    assert_eq!(number("0", 0.5), "1");
    assert_eq!(number("0", 2.5), "3");
    assert_eq!(number("0", -0.5), "-1");
    assert_eq!(number("0.00", 0.125), "0.13");
    assert_eq!(number("0%", 0.285), "29%");
}

#[test]
fn only_fifteen_digits_are_ever_shown() {
    assert_eq!(number("0", 1234567890123456.0), "1234567890123460");
    assert_eq!(number("0", 1e21), "1000000000000000000000");
    assert_eq!(
        number("0.00000000000000000000", 3.14159265358979),
        "3.14159265358979000000"
    );
}

#[test]
fn one_section_gives_negatives_a_minus_at_the_very_front() {
    assert_eq!(number("0", -5.0), "-5");
    assert_eq!(number("\"a\"0\"b\"", -5.0), "-a5b");
    assert_eq!(number("$0", -5.0), "-$5");
    assert_eq!(number("(0)", -5.0), "-(5)");
    assert_eq!(number("_(General_)", -5.0), "- 5 ");
    assert_eq!(number("\"abc\"", -5.0), "-abc");
}

#[test]
fn the_second_section_shows_negatives_without_a_minus() {
    assert_eq!(number("0;0", -5.0), "5");
    assert_eq!(number("0;-0", -5.0), "-5");
    assert_eq!(number("0;(0)", -5.0), "(5)");
    assert_eq!(number("General;General", -5.0), "5");
    assert_eq!(number("0;", -5.0), "");
}

#[test]
fn a_negative_that_rounds_to_zero_shows_no_minus() {
    assert_eq!(number("0", -0.4), "0");
    assert_eq!(number("0.00", -0.001), "0.00");
    assert_eq!(number("#", -0.4), "");
    // It is treated as the positive number, so it can land in another section.
    assert_eq!(number("[<=0]0", -0.4), "0.4");
    assert_eq!(number("[>100]0;[<=0]0;@", -0.4), "####");
    // The second section never had a sign to lose.
    assert_eq!(number("0;-0", -0.4), "-0");
    assert_eq!(number("0;(0)", -0.4), "(0)");
    // Without digits to round there is nothing to round away.
    assert_eq!(number("\"abc\"", -0.4), "-abc");
    assert_eq!(number("General", -0.4), "-0.4");
}

#[test]
fn plain_sections_are_positive_negative_zero_text() {
    let code = "\"p\";\"n\";\"z\";\"t\"";
    assert_eq!(number(code, 5.0), "p");
    assert_eq!(number(code, -5.0), "n");
    assert_eq!(number(code, 0.0), "z");
    assert_eq!(text(code, "abc"), "t");
    assert_eq!(number("0;0", 0.0), "0");
    assert_eq!(number("0;-0;\"z\"", 0.0), "z");
    assert_eq!(number("0;-0;\"z\"", 0.001), "0");
    assert_eq!(number(";;", 5.0), "");
    assert_eq!(number(";;;", 5.0), "");
    assert_eq!(number(";;0", 0.0), "0");
}

#[test]
fn a_condition_replaces_a_sections_test() {
    let code = "[>100]\"big\";[<=0]\"neg\";\"mid\"";
    assert_eq!(number(code, 101.0), "big");
    assert_eq!(number(code, 100.0), "mid");
    assert_eq!(number(code, 0.0), "neg");
    assert_eq!(number("[=1]\"one\";0", 1.0), "one");
    assert_eq!(number("[=1]\"one\";0", 2.0), "2");
    assert_eq!(number("[<>1]\"not\";0", 2.0), "not");
    assert_eq!(number("[>=1]0;[<1]0.00", 0.5), "0.50");
    assert_eq!(number("[>1E3]0;0.0", 1500.0), "1500");
    assert_eq!(number("[>999999]#,,\"M\";[>999]#,\"K\";#", 1234567.0), "1M");
    assert_eq!(number("[>999999]#,,\"M\";[>999]#,\"K\";#", 1234.0), "1K");
    assert_eq!(number("[>999999]#,,\"M\";[>999]#,\"K\";#", 12.0), "12");
}

#[test]
fn a_lone_condition_leaves_everything_else_to_general() {
    assert_eq!(number("[>100]0", 1234.5), "1235");
    assert_eq!(number("[>100]0", 1.5), "1.5");
    assert_eq!(number("[>100]0", -1.5), "-1.5");
    // General itself has nothing to fall back on.
    assert_eq!(number("[<0]General", 1.0), "####");
    assert_eq!(number("[<0]General", -1.0), "-1");
}

#[test]
fn a_number_no_section_takes_shows_as_hashes() {
    assert_eq!(number("[>100]0;[<0]0", 50.0), "####");
    assert_eq!(number("[>100]0;[<0]0", 1234.0), "1234");
    assert_eq!(number("0;[>100]0", 0.0), "####");
    assert_eq!(number("0;[<0]0", 0.0), "####");
    let (out, display) = with(&NumFmt::parse("[>100]0;[<0]0"), Value::Number(50.0));
    assert!(display.hashes);
    assert_eq!(out, "");
}

#[test]
fn a_section_only_negatives_can_reach_drops_the_minus() {
    assert_eq!(number("[<0]0;0", -5.0), "5");
    assert_eq!(number("[<=-1]0;0", -5.0), "5");
    assert_eq!(number("[=-5]0;0", -5.0), "5");
    assert_eq!(number("[>0]0;[<0]0", -5.0), "5");
    // These can also take a number that isn't negative, so the minus stays.
    assert_eq!(number("[<=0]0;0", -5.0), "-5");
    assert_eq!(number("[<5]0;0", -5.0), "-5");
    assert_eq!(number("[>-9]0;0", -5.0), "-5");
    assert_eq!(number("[>0]0;[<=0]0", -5.0), "-5");
}

#[test]
fn what_a_condition_leaves_loses_its_minus_unless_the_limit_is_above_zero() {
    assert_eq!(number("[>0]0;0", -5.0), "5");
    assert_eq!(number("[>=0]0;0", -5.0), "5");
    assert_eq!(number("[>-9]0;0", -10.0), "10");
    assert_eq!(number("[<-9]0;0", -5.0), "5");
    assert_eq!(number("[>100]0;0", -5.0), "-5");
    assert_eq!(number("[>=1]0;0", -5.0), "-5");
    assert_eq!(number("[=-9]0;0", -5.0), "-5");
    assert_eq!(number("[=0]0;0", -5.0), "-5");
    // The third section always keeps it.
    assert_eq!(number("0;[>-9]0;\"c\"0", -10.0), "-c10");
    // With three sections and no second condition, the second is for negatives as usual.
    assert_eq!(number("[>100]\"a\"0;\"b\"0;\"c\"0", -5.0), "b5");
    assert_eq!(number("[>100]\"a\"0;\"b\"0;\"c\"0", 5.0), "c5");
}

#[test]
fn empty_sections_with_a_condition_or_colour_are_general() {
    assert_eq!(number("[>100];0", 123.5), "123.5");
    assert_eq!(number("[Red]", 1.5), "1.5");
    assert_eq!(number("[Red];;;", 1.5), "1.5");
    assert_eq!(number("[Red];;;", -1.5), "");
    // An empty literal is not an empty section.
    assert_eq!(number("[<0]\"\";0%", -5.0), "");
    assert_eq!(number("[<0]\"\";0%", 0.5), "50%");
}

#[test]
fn scientific_keeps_at_least_the_exponent_width() {
    assert_eq!(number("0.00E+00", 1234.5678), "1.23E+03");
    assert_eq!(number("0.00E+00", 0.0), "0.00E+00");
    assert_eq!(number("0.00E+00", 1e100), "1.00E+100");
    assert_eq!(number("0.00E+000", 12.0), "1.20E+001");
    assert_eq!(number("0E+0", 0.004), "4E-3");
    assert_eq!(number("0E+0", 1.7976931348623157e308), "2E+308");
    assert_eq!(number("0.00E+00", -1234.5678), "-1.23E+03");
}

#[test]
fn e_minus_only_shows_a_minus() {
    assert_eq!(number("0E-0", 1234.0), "1E3");
    assert_eq!(number("0.00E-00", 0.5), "5.00E-01");
}

#[test]
fn whole_placeholders_set_the_step_of_the_exponent() {
    assert_eq!(number("##0.0E+0", 12345.0), "12.3E+3");
    assert_eq!(number("##0.0E+0", 1234567.0), "1.2E+6");
    assert_eq!(number("##0.0E+0", 0.5), "500.0E-3");
    assert_eq!(number("##0.0E+0", 1e-10), "100.0E-12");
    assert_eq!(number("00E+00", 1234.0), "12E+02");
    assert_eq!(number("00E+00", 1234567.0), "01E+06");
    assert_eq!(number("####E+0", 1234567.0), "123E+4");
    assert_eq!(number("#,##0E+0", 1234.0), "1,234E+0");
}

#[test]
fn zero_in_scientific_fills_every_whole_placeholder() {
    assert_eq!(number("##0.0E+0", 0.0), "000.0E+0");
    assert_eq!(number("#E+0", 0.0), "0E+0");
    assert_eq!(number("?E+0", 0.0), "0E+0");
    assert_eq!(number("#.#E+0", 0.0), "0.E+0");
    assert_eq!(number("#,##0E+0", 0.0), "0,000E+0");
}

#[test]
fn scientific_ignores_percents_and_scaling_commas() {
    assert_eq!(number("0E+00%", 1.0), "1E+00%");
    assert_eq!(number("0%E+00", 1.0), "1%E+00");
    assert_eq!(number("0,E+0", 1234.0), "1E+3");
    assert_eq!(number("0.0E+0,", 1234.0), "1.2E+3");
}

#[test]
fn fractions_use_continued_fraction_convergents() {
    assert_eq!(number("# ?/?", 0.5), " 1/2");
    assert_eq!(number("# ?/?", 3.14159265358979), "3 1/7");
    assert_eq!(number("# ??/??", 3.14159265358979), "3  1/7 ");
    assert_eq!(number("# ??/??", 1234.5678), "1234 46/81");
    // 1/9 is nearer to 0.1 but is no convergent of it.
    assert_eq!(number("# ?/?", 0.1), "0    ");
    assert_eq!(number("# ?/?", 1.3), "1 1/3");
}

#[test]
fn a_fixed_denominator_is_not_reduced() {
    assert_eq!(number("# ?/4", 0.5), " 2/4");
    assert_eq!(number("# ??/16", 0.5), "  8/16");
    assert_eq!(number("# ?/10", 3.14159265358979), "3 1/10");
    assert_eq!(number("# ??/100", 0.285), " 29/100");
    assert_eq!(number("# ?/2", 0.25), " 1/2");
    assert_eq!(number("# ?/2", 0.125), "0    ");
}

#[test]
fn a_whole_number_blanks_the_fraction() {
    assert_eq!(number("# ?/?", 5.0), "5    ");
    assert_eq!(number("# ??/??", 5.0), "5      ");
    assert_eq!(number("# ?/?", 0.0), "0    ");
    assert_eq!(number("# #/#", 5.0), "5");
    assert_eq!(number("# #/#", 0.0), "0");
    assert_eq!(number("# ?/2", 5.0), "5    ");
    assert_eq!(number("\"x\"# ?/?\"y\"", 5.0), "x5    y");
    assert_eq!(number("# ?/?", 0.999), "1    ");
    // A 0 in the numerator keeps the fraction.
    assert_eq!(number("# 0/0", 5.0), "5 0/1");
    assert_eq!(number("# 0/0", 0.0), "0/1");
    assert_eq!(number("0 0/0", 0.0), "0 0/1");
}

#[test]
fn a_blank_whole_part_keeps_its_space_only_for_a_question_mark() {
    assert_eq!(number("# #/#", 0.5), "1/2");
    assert_eq!(number("# 0/0", 0.5), "1/2");
    assert_eq!(number("#,##0 ?/?", 0.5), "0 1/2");
    assert_eq!(number("0 ?/?", 0.5), "0 1/2");
}

#[test]
fn a_fraction_with_no_whole_part_is_improper() {
    assert_eq!(number("?/?", 1234.5678), "8642/7");
    assert_eq!(number("?/?", 5.0), "5/1");
    assert_eq!(number("?/?", 0.0), "0/1");
    assert_eq!(number("??/??", 1.0), " 1/1 ");
    assert_eq!(number("00/00", 0.5), "01/02");
    assert_eq!(number("?/16", 5.0), "80/16");
    assert_eq!(number("0/ 0", 0.5), "1/ 2");
    assert_eq!(number("0 /0", 0.5), "1 /2");
}

#[test]
fn an_improper_numerator_can_be_too_large_to_show() {
    assert_eq!(number("?/?", 1e9), "1000000000/1");
    assert_eq!(number("?/?", 2147483647.0), "####");
    assert_eq!(number("?/1", 32767.0), "32767/1");
    assert_eq!(number("?/1", 32768.0), "####");
    assert_eq!(number("?/16", 10000.0), "####");
    // A mixed fraction keeps its whole part apart, so it has no such limit.
    assert_eq!(number("# ?/?", 4294967296.0), "4294967296    ");
}

#[test]
fn a_negative_fraction_keeps_its_minus_even_when_it_rounds_to_nothing() {
    assert_eq!(number("# ?/?", -1234.5678), "-1234 4/7");
    assert_eq!(number("# ?/?", -0.004), "-0    ");
    assert_eq!(number("?/?", -0.004), "0/1");
}

#[test]
fn a_percent_in_a_fraction_scales_it() {
    assert_eq!(number("# ?/?%", 0.125), "12 1/2%");
}

#[test]
fn literals_come_quoted_escaped_or_bare() {
    assert_eq!(number("\"abc\"0", 5.0), "abc5");
    assert_eq!(number("\"a\"\"b\"0", 5.0), "ab5");
    assert_eq!(number("\\a0", 5.0), "a5");
    assert_eq!(number("0\\%", 5.0), "5%");
    assert_eq!(number("$0", 5.0), "$5");
    assert_eq!(number("+0", 5.0), "+5");
    assert_eq!(number("{0}", 5.0), "{5}");
    assert_eq!(number("<0>", 5.0), "<5>");
    assert_eq!(number("€0", 5.0), "€5");
    assert_eq!(number("0x", 5.0), "5x");
    // A literal digit stays what it is.
    assert_eq!(number("\"1\"0", 5.0), "15");
}

#[test]
fn padding_is_one_space() {
    assert_eq!(number("0_)", 5.0), "5 ");
    assert_eq!(number("_(0_)", 5.0), " 5 ");
    assert_eq!(number("0_x_y", 5.0), "5  ");
    assert_eq!(text("_(@_)", "abc"), " abc ");
}

#[test]
fn fill_is_left_to_the_worker_with_its_position() {
    let (out, display) = with(&NumFmt::parse("\"a\"*-0"), Value::Number(5.0));
    assert_eq!(out, "a5");
    assert_eq!(display.fill, Some(('-', 1)));
    let (out, display) = with(&NumFmt::parse("0*x"), Value::Number(5.0));
    assert_eq!(out, "5");
    assert_eq!(display.fill, Some(('x', 1)));
    let (out, display) = with(&NumFmt::parse("*x@"), Value::Text("abc"));
    assert_eq!(out, "abc");
    assert_eq!(display.fill, Some(('x', 0)));
}

#[test]
fn the_last_fill_wins() {
    let (out, display) = with(&NumFmt::parse("*-\"a\"*=0"), Value::Number(5.0));
    assert_eq!(out, "a5");
    assert_eq!(display.fill, Some(('=', 1)));
}

#[test]
fn accounting_formats_line_up_with_fill_and_padding() {
    let accounting = NumFmt::builtin(43).unwrap();
    let (out, display) = with(&accounting, Value::Number(-1234.5));
    assert_eq!(out, " (1,234.50)");
    assert_eq!(display.fill, Some((' ', 1)));
    let (out, _) = with(&accounting, Value::Number(0.0));
    assert_eq!(out, " -   ");
    let (out, _) = with(&NumFmt::builtin(44).unwrap(), Value::Number(5.0));
    assert_eq!(out, " $5.00 ");
}

#[test]
fn a_position_is_in_bytes_and_counts_what_was_already_in_out() {
    let mut out = String::from("ab");
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: false,
    };
    let display = NumFmt::parse("€*-0").format(Value::Number(5.0), &options, &mut out);
    assert_eq!(out, "ab€5");
    assert_eq!(display.fill, Some(('-', 5)));
}

#[test]
fn text_goes_through_the_text_section_or_shows_as_itself() {
    assert_eq!(text("0", "abc"), "abc");
    assert_eq!(text("0;0;0", "abc"), "abc");
    assert_eq!(text("yyyy", "abc"), "abc");
    assert_eq!(text("@", "abc"), "abc");
    assert_eq!(text("\"a\"@\"b\"", "abc"), "aabcb");
    assert_eq!(text("@@", "abc"), "abcabc");
    assert_eq!(text("@ @", "abc"), "abc abc");
    assert_eq!(text("0;@", "abc"), "abc");
    assert_eq!(text("0;0;0;\"text\"", "abc"), "text");
    assert_eq!(text("0;0;0;", "abc"), "");
    assert_eq!(text(";;;", "abc"), "");
    assert_eq!(text(";;", "abc"), "abc");
    assert_eq!(text("General;General;General;General", "abc"), "abc");
}

#[test]
fn numbers_ignore_a_text_section_and_a_lone_at_means_general() {
    assert_eq!(number("@", 1234.5678), "1234.5678");
    assert_eq!(number("\"a\"@\"b\"", 5.0), "5");
    assert_eq!(number("0;@", -5.0), "-5");
    assert_eq!(number("0;0;@", -5.0), "5");
    assert_eq!(number("0;0;0;@", -5.0), "5");
}

#[test]
fn booleans_show_their_name_and_take_a_text_section() {
    let shown = |code: &str, flag| with(&NumFmt::parse(code), Value::Bool(flag)).0;
    assert_eq!(shown("0", true), "TRUE");
    assert_eq!(shown("yyyy", false), "FALSE");
    assert_eq!(shown("0.00E+00", false), "FALSE");
    // The one place a format reaches them.
    assert_eq!(shown("\"a\"@\"b\"", true), "aTRUEb");
    assert_eq!(shown("@@", true), "TRUETRUE");
    assert_eq!(shown(";;;", true), "");
    assert_eq!(shown("\"p\";\"n\";\"z\";\"t\"", false), "t");
}

#[test]
fn errors_ignore_the_format() {
    let shown = |code: &str| with(&NumFmt::parse(code), Value::Error("#N/A")).0;
    assert_eq!(shown("0"), "#N/A");
    assert_eq!(shown(";;;"), "#N/A");
    assert_eq!(shown("\"a\"@\"b\""), "#N/A");
}

#[test]
fn a_section_gives_its_colour_to_what_it_shows() {
    let color = |code: &str, value| with(&NumFmt::parse(code), value).1.color;
    let code = "[Red]0;[Blue]0;[Green]0;[Yellow]@";
    assert_eq!(color(code, Value::Number(5.0)), Some(FormatColor::Red));
    assert_eq!(color(code, Value::Number(-5.0)), Some(FormatColor::Blue));
    assert_eq!(color(code, Value::Number(0.0)), Some(FormatColor::Green));
    assert_eq!(color(code, Value::Text("x")), Some(FormatColor::Yellow));
    assert_eq!(color(code, Value::Bool(true)), Some(FormatColor::Yellow));
    assert_eq!(color("[Red]0", Value::Number(0.0)), Some(FormatColor::Red));
    assert_eq!(color("0", Value::Number(5.0)), None);
    // Only a text section or a lone General colours text.
    assert_eq!(color("[Red]0", Value::Text("x")), None);
    assert_eq!(color("[Red]@", Value::Number(5.0)), None);
    assert_eq!(color("[Red]@", Value::Text("x")), Some(FormatColor::Red));
    assert_eq!(
        color("[Red]General", Value::Text("x")),
        Some(FormatColor::Red)
    );
}

#[test]
fn colours_come_by_name_or_number() {
    let color = |code: &str| {
        with(&NumFmt::parse(code), Value::Number(5.0))
            .1
            .color
            .unwrap()
    };
    assert_eq!(color("[Black]0"), FormatColor::Black);
    assert_eq!(color("[Cyan]0"), FormatColor::Cyan);
    assert_eq!(color("[Magenta]0"), FormatColor::Magenta);
    assert_eq!(color("[White]0"), FormatColor::White);
    assert_eq!(color("[RED]0"), FormatColor::Red);
    assert_eq!(color("[red]0"), FormatColor::Red);
    assert_eq!(color("[Color1]0"), FormatColor::Indexed(1));
    assert_eq!(color("[Color 10]0"), FormatColor::Indexed(10));
    assert_eq!(color("[Color56]0"), FormatColor::Indexed(56));
    assert_eq!(color("[Red][>1]0"), FormatColor::Red);
    assert_eq!(color("[>1][Red]0"), FormatColor::Red);
}

#[test]
fn date_tokens() {
    // Friday 1 March 2024, noon.
    let at = 45352.5;
    assert_eq!(number("y", at), "24");
    assert_eq!(number("yy", at), "24");
    assert_eq!(number("yyy", at), "2024");
    assert_eq!(number("yyyy", at), "2024");
    assert_eq!(number("m", at), "3");
    assert_eq!(number("mm", at), "03");
    assert_eq!(number("mmm", at), "Mar");
    assert_eq!(number("mmmm", at), "March");
    assert_eq!(number("mmmmm", at), "M");
    assert_eq!(number("mmmmmm", at), "March");
    assert_eq!(number("d", at), "1");
    assert_eq!(number("dd", at), "01");
    assert_eq!(number("ddd", at), "Fri");
    assert_eq!(number("dddd", at), "Friday");
    assert_eq!(number("ddddd", at), "Friday");
    assert_eq!(number("h", at), "12");
    assert_eq!(number("hh", 45352.25), "06");
    assert_eq!(number("hhh", 45352.25), "06");
    assert_eq!(number("D/M/YYYY", at), "1/3/2024");
    assert_eq!(number("HH:MM", at), "12:00");
    assert_eq!(number("yyyy-mm-dd", at), "2024-03-01");
    assert_eq!(number("\"on\" d", at), "on 1");
}

#[test]
fn the_1900_calendar_has_a_day_zero_and_a_leap_day_that_never_was() {
    assert_eq!(number("yyyy-mm-dd ddd", 0.0), "1900-01-00 Sat");
    assert_eq!(number("yyyy-mm-dd ddd", 1.0), "1900-01-01 Sun");
    assert_eq!(number("yyyy-mm-dd ddd", 59.0), "1900-02-28 Tue");
    assert_eq!(number("yyyy-mm-dd ddd", 60.0), "1900-02-29 Wed");
    assert_eq!(number("yyyy-mm-dd ddd", 61.0), "1900-03-01 Thu");
    assert_eq!(number("yyyy-mm-dd ddd", 2958465.0), "9999-12-31 Fri");
}

#[test]
fn a_date_out_of_range_shows_as_hashes() {
    assert_eq!(number("yyyy", -1.0), "####");
    assert_eq!(number("h:mm", -0.5), "####");
    assert_eq!(number("yyyy", 2958466.0), "####");
    assert_eq!(number("yyyy", 2958465.99999), "9999");
    assert_eq!(number("yyyy", 2958465.999999), "####");
    // Only the section it lands in matters.
    assert_eq!(number("yyyy;0", -1.0), "1");
    assert_eq!(number("0;yyyy", -1.0), "####");
}

#[test]
fn m_is_minutes_after_hours_or_seconds_and_before_seconds() {
    let at = 2958465.99999; // 12/31/9999 23:59:59
    assert_eq!(number("h:m", at), "23:59");
    assert_eq!(number("h:mm", at), "23:59");
    assert_eq!(number("mm:ss", at), "59:59");
    assert_eq!(number("h\"x\"m", at), "23x59");
    assert_eq!(number("s m", at), "59 59");
    assert_eq!(number("m \"x\" s", at), "59 x 59");
    assert_eq!(number("h AM/PM m", at), "11 PM 59");
    // A date token in between doesn't count as being next to seconds, but does not stop hours.
    assert_eq!(number("m d s", at), "12 31 59");
    assert_eq!(number("h d m", at), "23 31 59");
    // Minutes use up the hours before them.
    assert_eq!(number("h:mm m", at), "23:59 12");
    assert_eq!(number("m s m", at), "59 59 12");
    assert_eq!(number("mm h", at), "12 23");
    // Names are never minutes.
    assert_eq!(number("h mmm", at), "23 Dec");
}

#[test]
fn am_and_pm() {
    assert_eq!(number("h AM/PM", 0.75), "6 PM");
    assert_eq!(number("h AM/PM", 0.0), "12 AM");
    assert_eq!(number("h am/pm", 0.5), "12 PM");
    assert_eq!(number("h:mm:ss A/P", 0.75), "6:00:00 P");
    // A/P keeps the case it was written in, letter by letter.
    assert_eq!(number("h a/p", 0.25), "6 a");
    assert_eq!(number("h A/p", 0.75), "6 p");
    assert_eq!(number("h a/P", 0.75), "6 P");
    assert_eq!(number("AM/PM h", 0.75), "PM 6");
}

#[test]
fn fractions_of_a_second_take_up_to_three_digits() {
    assert_eq!(number("ss.000", 45000.999999), "59.914");
    assert_eq!(number("ss.0", 45000.999999), "59.9");
    assert_eq!(number("mm:ss.0", 0.0000115740740740741), "00:01.0");
    assert_eq!(number("h:mm:ss.000 AM/PM", 45000.999999), "11:59:59.914 PM");
    assert_eq!(number("[s].00", 45000.999999), "3888086399.91");
}

#[test]
fn time_rounds_to_the_second_shown_and_carries_into_the_date() {
    assert_eq!(number("h:mm:ss", 0.999994), "23:59:59");
    assert_eq!(number("h:mm:ss", 0.999995), "0:00:00");
    assert_eq!(number("h:mm", 0.999994), "23:59");
    assert_eq!(number("h:mm", 0.999995), "0:00");
    assert_eq!(
        number("yyyy-mm-dd hh:mm:ss", 45000.999995),
        "2023-03-16 00:00:00"
    );
    assert_eq!(number("yyyy-mm-dd", 45000.999995), "2023-03-16");
    assert_eq!(number("yyyy-mm-dd", 0.999995), "1900-01-01");
    assert_eq!(number("yyyy", 2958465.999995), "####");
}

#[test]
fn elapsed_time_counts_past_the_clock() {
    assert_eq!(number("[h]", 2.0), "48");
    assert_eq!(number("[h]:mm:ss", 1.75), "42:00:00");
    assert_eq!(number("[m]", 2.0), "2880");
    assert_eq!(number("[m]:ss", 0.0000115740740740741), "0:01");
    assert_eq!(number("[s]", 2.0), "172800");
    assert_eq!(number("[h]:mm", 0.999995), "24:00");
    assert_eq!(number("[s]", 45000.999999), "3888086400");
    assert_eq!(number("[hh]", 0.0), "00");
    assert_eq!(number("[hhh]", 2.0), "048");
    assert_eq!(number("[mmm]", 0.0), "000");
    assert_eq!(number("[H]", 2.0), "48");
    assert_eq!(number("[h] h", -1.0), "-24 24");
}

#[test]
fn elapsed_time_alone_has_no_range_but_beside_anything_else_it_does() {
    assert_eq!(number("[h]", -1.0), "-24");
    assert_eq!(number("[m]", -0.5), "-720");
    assert_eq!(number("-[h]", -1.0), "--24");
    assert_eq!(number("\"x\"[h]", -1.0), "-x24");
    assert_eq!(number("[h]", 2958466.0), "71003184");
    assert_eq!(number("[h]", 1e11), "2400000000000");
    assert_eq!(number("[h]:mm", -1.0), "####");
    assert_eq!(number("[h]:mm", 2958466.0), "####");
}

#[test]
fn buddhist_years_and_eras_without_a_tag() {
    assert_eq!(number("b", 45292.0), "67");
    assert_eq!(number("bb", 45292.0), "67");
    assert_eq!(number("bbb", 45292.0), "2567");
    assert_eq!(number("bbbb", 45292.0), "2567");
    assert_eq!(number("e", 45292.0), "2024");
    assert_eq!(number("ee", 45292.0), "2024");
    assert_eq!(number("g", 45292.0), "");
    assert_eq!(number("ggg", 45292.0), "");
    assert_eq!(number("b1", 45292.0), "");
    assert_eq!(number("b2\"x\"", 45292.0), "x");
}

#[test]
fn with_a_tag_b_is_only_the_year() {
    assert_eq!(number("[$-409]b", 45292.0), "24");
    assert_eq!(number("[$-401]bbbb", 45292.0), "2024");
    assert_eq!(number("[$-60401]b", 45292.0), "45");
}

#[test]
fn japanese_eras() {
    assert_eq!(
        number("[$-411]ggge\"年\"m\"月\"d\"日\"", 45292.0),
        "令和6年1月1日"
    );
    assert_eq!(number("[$-411]e", 45292.0), "6");
    assert_eq!(number("[$-411]ee", 45292.0), "06");
    assert_eq!(number("[$-411]g", 45292.0), "R");
    assert_eq!(number("[$-411]gg", 45292.0), "令");
    assert_eq!(number("[$-411]yyyy", 45292.0), "2024");
    assert_eq!(number("[$-411]ggge", 32874.0), "平成2");
    assert_eq!(number("[$]ggge", 45292.0), "2024");
}

#[test]
fn date1904_starts_in_1904_and_shows_negatives_with_a_minus() {
    assert_eq!(date1904("yyyy-mm-dd dddd", 0.0), "1904-01-01 Friday");
    assert_eq!(date1904("yyyy-mm-dd", 1.0), "1904-01-02");
    assert_eq!(date1904("yyyy-mm-dd", 59.0), "1904-02-29");
    assert_eq!(date1904("yyyy-mm-dd", 2957003.0), "9999-12-31");
    assert_eq!(date1904("yyyy", 2957004.0), "####");
    assert_eq!(date1904("[h]:mm:ss", -1.0), "-24:00:00");
    assert_eq!(date1904("yyyy-mm-dd", -1.0), "-1904-01-02");
    assert_eq!(date1904("b2yyyy/mm/dd", 0.0), "1321/10/13");
}

#[test]
fn a_tag_sets_the_names_and_always_wins_over_the_locale() {
    let at = 45352.0;
    assert_eq!(number("[$-409]mmmm dddd", at), "March Friday");
    assert_eq!(number("[$-401]dddd d mmmm yyyy", at), "الجمعة 1 مارس 2024");
    assert_eq!(number("[$-C01]mmm ddd", at), "مارس الجمعة");
    assert_eq!(
        number("[$-40C]dddd d mmmm yyyy", at),
        "vendredi 1 mars 2024"
    );
    assert_eq!(number("[$-40C]ddd d mmm", at), "ven. 1 mars");
    assert_eq!(number("[$-407]dddd d mmmm yyyy", at), "Freitag 1 März 2024");
    assert_eq!(number("[$-407]ddd d mmm", at), "Fr 1 Mrz");
    assert_eq!(
        number("[$-C0A]dddd d mmmm yyyy", at),
        "viernes 1 marzo 2024"
    );
    assert_eq!(number("[$-40A]ddd d mmm", at), "vi. 1 mar.");
    // A tag that names no language leaves the locale's names.
    assert_eq!(number("[$-1000000]dddd", at), "Friday");
    // A language not in the table gets English.
    assert_eq!(number("[$-41D]mmmm", at), "March");
}

#[test]
fn iraq_and_the_levant_have_their_own_month_names() {
    assert_eq!(number("[$-801]mmmm", 45306.0), "كانون\u{a0}الثاني");
    assert_eq!(number("[$-2801]mmmm", 45366.0), "آذار");
    assert_eq!(number("[$-2C01]mmmm", 45397.0), "نيسان");
    assert_eq!(number("[$-3001]mmmm", 45550.0), "أيلول");
    assert_eq!(number("[$-401]mmmm", 45306.0), "يناير");
    assert_eq!(number("[$-3801]mmmm", 45306.0), "يناير");
}

#[test]
fn am_and_pm_follow_a_language_tag() {
    assert_eq!(number("[$-401]h:mm AM/PM", 0.25), "6:00 ص");
    assert_eq!(number("[$-401]h:mm AM/PM", 0.75), "6:00 م");
    assert_eq!(number("[$-401]h A/P", 0.75), "6 م");
    assert_eq!(number("[$-409]h A/P", 0.75), "6 PM");
}

#[test]
fn only_the_first_section_has_a_say_in_the_tag() {
    assert_eq!(number("[$-401]dddd;[$-409]dddd", 45352.0), "الجمعة");
    assert_eq!(number("[$-409]dddd;[$-401]dddd", 45352.0), "Friday");
    assert_eq!(number("dddd;[$-401]dddd", 45352.0), "Friday");
    assert_eq!(number("0;[$-2000401]0", -5.0), "5");
    assert_eq!(number("[$-2000401]0;[$-3000401]0", -5.0), "٥");
    // Anywhere in the first section will do.
    assert_eq!(number("dddd[$-401]", 45352.0), "الجمعة");
}

#[test]
fn a_tag_on_a_text_section_does_nothing() {
    assert_eq!(number("[$-2000401]@", 12.0), "12");
    assert_eq!(number("[$-401]0;[$-2000401]@", 12.0), "12");
}

#[test]
fn a_currency_tag_shows_its_symbol_and_can_carry_a_language() {
    assert_eq!(number("[$€-407]#,##0.00", 1234.5), "€1,234.50");
    assert_eq!(number("[$$-409]#,##0", 1234.5), "$1,235");
    assert_eq!(number("#,##0.00 [$€-1]", 1234.5), "1,234.50 €");
    assert_eq!(number("[$USD]0", 5.0), "USD5");
    assert_eq!(number("[$AED-3801]0.00", 5.0), "AED5.00");
    assert_eq!(number("[$€]0", -5.0), "-€5");
    assert_eq!(number("[$-]0", 5.0), "5");
    assert_eq!(number("[$]0", 5.0), "5");
    assert_eq!(number("[$€-407]dddd", 45292.0), "€Montag");
    assert_eq!(number("[$€-401]dddd", 45292.0), "€الإثنين");
    assert_eq!(number("[$$-2000401]0", 12.0), "$١٢");
    assert_eq!(number("[$€-60401]yyyy", 45292.0), "€1445");
    assert_eq!(text("[$€-407]@", "abc"), "€abc");
}

#[test]
fn separators_stay_the_systems_whatever_the_tag() {
    assert_eq!(number("[$-407]#,##0.00", 1234.5), "1,234.50");
    assert_eq!(number("[$-40C]0.00", 1.5), "1.50");
}

#[test]
fn numeral_shapes_change_only_the_digits_a_number_produces() {
    assert_eq!(number("[$-2000401]0", 1234.5678), "١٢٣٥");
    assert_eq!(number("[$-3000401]0", 1234.5678), "۱۲۳۵");
    assert_eq!(number("[$-1000401]0", 1234.5678), "1235");
    assert_eq!(number("[$-2000409]0", 12.0), "١٢");
    assert_eq!(number("[$-2000000]0", 12.0), "١٢");
    assert_eq!(number("[$-4000000]0", 12.0), "१२");
    assert_eq!(number("[$-2000401]#,##0.00", 1234.5678), "١,٢٣٤.٥٧");
    assert_eq!(number("[$-2000401]0%", 0.5), "٥٠%");
    assert_eq!(number("[$-2000401]0.00E+00", 12.0), "١.٢٠E+٠١");
    assert_eq!(number("[$-2000401]General", 1234.5678), "١٢٣٤.٥٦٧٨");
    assert_eq!(number("[$-2000401]yyyy/mm/dd", 45352.0), "٢٠٢٤/٠٣/٠١");
    assert_eq!(number("[$-2000401]h:mm:ss AM/PM", 0.5), "١٢:٠٠:٠٠ م");
    assert_eq!(number("[$-2000401][h]:mm", 1.5), "٣٦:٠٠");
    assert_eq!(
        number("[$-2000401]dddd d mmmm yyyy", 45352.0),
        "الجمعة ١ مارس ٢٠٢٤"
    );
    // Literals keep theirs, even when they are digits.
    assert_eq!(number("[$-2000401]\"1\"0", 2.0), "1٢");
    assert_eq!(number("[$-2000401]\\10", 2.0), "1٢");
    // An Excel oddity: this one zero isn't shaped.
    assert_eq!(number("[$-2000401]# ?/?", 0.0), "0    ");
    assert_eq!(number("[$-2000401]# ?/?", 12.0), "١٢    ");
}

#[test]
fn hijri_dates_use_the_tabular_kuwaiti_calendar() {
    assert_eq!(number("b2yyyy/mm/dd", 45292.0), "1445/06/20");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 45292.0), "1445/06/20");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 45352.5), "1445/08/21");
    assert_eq!(number("[$-60401]yy/m/d", 366.0), "18/9/10");
    assert_eq!(number("[$-60401]e/m/d", 2958465.0), "9666/4/3");
    assert_eq!(
        number("[$-60401]yyyy/mm/dd hh:mm:ss", 45352.5),
        "1445/08/21 12:00:00"
    );
    assert_eq!(number("[$-2060401]yyyy/mm/dd", 45292.0), "١٤٤٥/٠٦/٢٠");
    assert_eq!(number("[$-3060401]yyyy/mm/dd", 45292.0), "۱۴۴۵/۰۶/۲۰");
}

#[test]
fn hijri_has_no_day_zero_and_no_false_leap_day() {
    assert_eq!(number("[$-60401]yyyy/mm/dd", 0.0), "1317/08/29");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 1.0), "1317/08/29");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 2.0), "1317/09/01");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 59.0), "1317/10/28");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 60.0), "####");
    assert_eq!(number("[$-60401]yyyy/mm/dd", 61.0), "1317/10/29");
    assert_eq!(number("b2yyyy", 60.0), "####");
}

#[test]
fn hijri_month_and_day_names() {
    assert_eq!(number("[$-60401]mmmm", 45292.0), "جمادى\u{a0}الثانية");
    assert_eq!(number("[$-60401]mmm", 45488.0), "محرم");
    assert_eq!(number("[$-60401]mmmmm", 45488.0), "م");
    assert_eq!(number("[$-60401]dddd", 45292.0), "الإثنين");
    assert_eq!(number("b2mmmm", 45292.0), "جمادى\u{a0}الثانية");
    assert_eq!(number("[$-60000]mmmm", 45292.0), "جمادى\u{a0}الثانية");
    assert_eq!(number("[$-60C01]mmmm", 45292.0), "جمادى\u{a0}الآخرة");
    assert_eq!(number("[$-60409]mmmm", 45292.0), "Jamada El Thaniah");
    assert_eq!(number("[$-60409]dddd", 45292.0), "AlEthnien");
    assert_eq!(number("[$-60401]h:mm AM/PM", 0.75), "6:00 م");
    assert_eq!(number("[$-60409]h:mm AM/PM", 0.75), "6:00 PM");
}

#[test]
fn a_calendar_prefix_after_a_tag_does_nothing() {
    assert_eq!(number("[$-401]b2yyyy/mm/dd", 45292.0), "2024/01/01");
    assert_eq!(number("[$-409]b2yyyy", 45292.0), "2024");
    assert_eq!(number("[$-60401]b1yyyy", 45292.0), "1445");
    assert_eq!(number("b2[$-401]yyyy/mm/dd", 45292.0), "1445/06/20");
    assert_eq!(number("b1yyyy", 45292.0), "2024");
}

#[test]
fn other_calendars_in_the_long_form() {
    let at = 45352.0;
    // 01 is localized, 02 is US English.
    assert_eq!(number("[$-10401]mmmm yyyy", at), "مارس 2024");
    assert_eq!(number("[$-20401]mmmm yyyy", at), "March 2024");
    assert_eq!(number("[$-20401]yyyy/mm/dd", 0.0), "1900/01/00");
    assert_eq!(number("[$-20401]h AM/PM", 0.75), "6 م");
    // 0A is Gregorian Arabic: Levantine names, and the calendar has no day 0 or false leap day.
    assert_eq!(number("[$-A0401]mmmm yyyy", at), "آذار 2024");
    assert_eq!(number("[$-A0401]yyyy/mm/dd", 0.0), "1900/01/01");
    assert_eq!(number("[$-A0401]yyyy/mm/dd", 60.0), "####");
    // 03 is the Japanese emperor's calendar.
    assert_eq!(number("[$-30409]yyyy", at), "06");
    assert_eq!(number("[$-30409]e", at), "6");
    assert_eq!(number("[$-30409]ggg", at), "令和");
    // 07 is the Thai Buddhist calendar.
    assert_eq!(number("[$-70409]yyyy", at), "2567");
    // A leading zero in the tag changes nothing.
    assert_eq!(number("[$-00000409]dddd", at), "Friday");
    assert_eq!(number("[$-0409]0", 5.0), "5");
}

#[test]
fn language_tags_can_be_written_out() {
    assert_eq!(number("[$-en-US]dddd", 45352.0), "Friday");
    assert_eq!(number("[$-ar-SA]dddd", 45352.0), "الجمعة");
    assert_eq!(number("[$-ar-SA,1]dddd", 45352.0), "الجمعة");
    assert_eq!(number("[$-fr-FR]dddd", 45352.0), "vendredi");
    assert_eq!(
        number("[$-x-sysdate]yyyy", 45352.0),
        "Friday, March 1, 2024"
    );
    assert_eq!(number("[$-x-systime]yyyy", 0.75), "6:00:00 PM");
}

#[test]
fn system_tags_replace_the_whole_section_with_the_locales_form() {
    assert_eq!(number("[$-F800]dddd", 45352.0), "Friday, March 1, 2024");
    assert_eq!(number("[$-F800]0", 45352.0), "Friday, March 1, 2024");
    assert_eq!(number("[$-F400]h:mm", 0.75), "6:00:00 PM");
    assert_eq!(number("[$-F400]0", 0.75), "6:00:00 PM");
    assert_eq!(number("[$-F800]dddd", -1.0), "####");
    assert_eq!(text("[$-F800]dddd", "abc"), "abc");
    assert!(NumFmt::parse("[$-F800]0").is_date());
}

#[test]
fn built_in_ids_are_excels_formats() {
    let shown = |id: u32, value: f64| {
        let format = NumFmt::builtin(id).unwrap();
        assert!(format.is_valid(), "built-in {id}");
        with(&format, Value::Number(value)).0
    };
    assert_eq!(shown(0, 1234.5), "1234.5");
    assert_eq!(shown(1, 1234.5), "1235");
    assert_eq!(shown(2, 1234.5), "1234.50");
    assert_eq!(shown(3, 1234.5), "1,235");
    assert_eq!(shown(4, 1234.5), "1,234.50");
    assert_eq!(shown(5, -1234.5), "($1,235)");
    assert_eq!(shown(6, 1234.5), "$1,235 ");
    assert_eq!(shown(7, 1234.5), "$1,234.50 ");
    assert_eq!(shown(8, -1234.5), "($1,234.50)");
    assert_eq!(shown(9, 0.5), "50%");
    assert_eq!(shown(10, 0.5), "50.00%");
    assert_eq!(shown(11, 1234.5), "1.23E+03");
    assert_eq!(shown(12, 1.5), "1 1/2");
    assert_eq!(shown(13, 1.5), "1  1/2 ");
    assert_eq!(shown(14, 45352.0), "3/1/2024");
    assert_eq!(shown(15, 45352.0), "1-Mar-24");
    assert_eq!(shown(16, 45352.0), "1-Mar");
    assert_eq!(shown(17, 45352.0), "Mar-24");
    assert_eq!(shown(18, 0.75), "6:00 PM");
    assert_eq!(shown(19, 0.75), "6:00:00 PM");
    assert_eq!(shown(20, 0.75), "18:00");
    assert_eq!(shown(21, 0.75), "18:00:00");
    assert_eq!(shown(22, 45352.75), "3/1/2024 18:00");
    assert_eq!(shown(37, -1234.5), "(1,235)");
    assert_eq!(shown(38, 1234.5), "1,235 ");
    assert_eq!(shown(39, -1234.5), "(1,234.50)");
    assert_eq!(shown(40, 1234.5), "1,234.50 ");
    assert_eq!(shown(41, 1234.5), " 1,235 ");
    assert_eq!(shown(42, -1234.5), " $(1,235)");
    assert_eq!(shown(45, 0.5007), "01:00");
    assert_eq!(shown(46, 1.75), "42:00:00");
    assert_eq!(shown(47, 0.0000115740740740741), "00:01.0");
    assert_eq!(shown(48, 12345.0), "12.3E+3");
    assert_eq!(shown(49, 1234.5), "1234.5");
    assert_eq!(
        with(&NumFmt::builtin(49).unwrap(), Value::Text("abc")).0,
        "abc"
    );
    assert!(NumFmt::builtin(14).unwrap().is_date());
    assert!(NumFmt::builtin(0).unwrap().is_general());
    assert!(NumFmt::builtin(1000).is_none());
}

#[test]
fn the_higher_ids_excel_keeps_are_copies_of_lower_ones() {
    let same = |low: u32, high: u32, value: f64| {
        let low = with(&NumFmt::builtin(low).unwrap(), Value::Number(value)).0;
        let high = with(&NumFmt::builtin(high).unwrap(), Value::Number(value)).0;
        assert_eq!(low, high);
    };
    same(0, 23, 1234.5);
    same(14, 27, 45352.0);
    same(21, 32, 0.75);
    same(14, 50, 45352.0);
    same(1, 59, 1234.5);
    same(5, 63, -1234.5);
    same(12, 69, 1.5);
    same(22, 78, 45352.75);
    same(47, 81, 0.5);
}

fn german() -> Locale {
    Locale {
        decimal: ",".to_string(),
        group: ".".to_string(),
        short_date: "dd.mm.yyyy".to_string(),
        long_date: "dddd, d. mmmm yyyy".to_string(),
        short_time: "hh:mm".to_string(),
        long_time: "hh:mm:ss".to_string(),
        am: "vorm.".to_string(),
        pm: "nachm.".to_string(),
        months: Locale::en_us().months,
        months_short: Locale::en_us().months_short,
        days: Locale::en_us().days,
        days_short: Locale::en_us().days_short,
        currency: "€".to_string(),
        date_separator: ".".to_string(),
        time_separator: ":".to_string(),
    }
}

#[test]
fn the_locale_supplies_separators_regional_forms_and_the_currency_symbol() {
    let locale = german();
    let options = Options {
        locale: &locale,
        date1904: false,
    };
    let shown = |format: NumFmt, value: f64| {
        let mut out = String::new();
        format.format(Value::Number(value), &options, &mut out);
        out
    };
    assert_eq!(shown(NumFmt::parse("#,##0.00"), 1234.5), "1.234,50");
    assert_eq!(shown(NumFmt::parse("0.0E+00"), 1234.5), "1,2E+03");
    assert_eq!(shown(NumFmt::parse("General"), 1234.5), "1234,5");
    assert_eq!(shown(NumFmt::parse("ss.0"), 0.5), "00,0");
    assert_eq!(shown(NumFmt::parse("dd/mm/yyyy"), 45352.0), "01.03.2024");
    assert_eq!(shown(NumFmt::parse("dd\\/mm"), 45352.0), "01/03");
    assert_eq!(shown(NumFmt::parse("h AM/PM"), 0.75), "6 nachm.");
    assert_eq!(shown(NumFmt::builtin(14).unwrap(), 45352.0), "01.03.2024");
    assert_eq!(
        shown(NumFmt::builtin(22).unwrap(), 45352.75),
        "01.03.2024 18:00"
    );
    assert_eq!(shown(NumFmt::builtin(7).unwrap(), 1234.5), "€1.234,50 ");
    assert_eq!(shown(NumFmt::parse("[$-F400]0"), 0.75), "18:00:00");
    // A tag still wins.
    assert_eq!(shown(NumFmt::parse("[$-409]h AM/PM"), 0.75), "6 PM");
    // A literal $ stays one.
    assert_eq!(shown(NumFmt::parse("$0"), 5.0), "$5");
}

#[test]
fn what_excel_refuses_to_read_is_general_and_says_so() {
    for code in [
        "\"unterminated",
        "\\",
        "_",
        "*",
        "0\\",
        "0_",
        "0*",
        "[Red",
        "[Foo]0",
        "[Orange]0",
        "[]0",
        "[100]0",
        "[Color]0",
        "[Color0]0",
        "[Color57]0",
        "[Red][Blue]0",
        "[>1][<2]0",
        "0;0;0;0;0",
        ";;;;",
        "0;0;[>100]0",
        "@;0",
        "@;@",
        "0;@;0",
        "0@",
        "@0",
        "General 0",
        "0 General",
        "General General",
        "General@",
        "0:0",
        "0/",
        "/0",
        "0.0/0",
        "E+0",
        "0E",
        "0E+",
        "0E+0E+0",
        "0.00e+00",
        "n",
        "0n",
        "0d",
        "0m",
        "0y",
        "0 kg",
        "yyyy 0",
        "0 yyyy",
        "yyyy%",
        "yyyy@",
        "h:mm:ss.0000",
        "ss.#",
        "[h]:[m]",
        "h:[m]",
        "[h] AM/PM",
        "b3yyyy",
        "yyyy b2",
        "b20",
        "B",
        "E",
    ] {
        let format = NumFmt::parse(code);
        assert!(!format.is_valid(), "{code} should not be valid");
        assert!(format.is_general(), "{code} should behave as General");
        let (out, display) = with(&format, Value::Number(1234.5));
        assert_eq!(out, "1234.5", "{code}");
        assert!(display.general, "{code}");
    }
}

#[test]
fn bare_letters_excel_reads_as_themselves() {
    for (code, want) in [
        ("x", "x"),
        ("0x", "5x"),
        ("0a", "5a"),
        ("0k", "5k"),
        ("#x", "5x"),
        ("d x", "0 x"),
    ] {
        let value = if code.starts_with('d') { 0.0 } else { 5.0 };
        assert_eq!(number(code, value), want, "{code}");
    }
}

#[test]
fn what_kind_of_format_it_is() {
    assert!(NumFmt::parse("yyyy").is_date());
    assert!(NumFmt::parse("[h]:mm").is_date());
    assert!(NumFmt::parse("AM/PM").is_date());
    assert!(NumFmt::parse("0;yyyy").is_date());
    assert!(!NumFmt::parse("0.00").is_date());
    assert!(!NumFmt::parse("\"yyyy\"").is_date());
    assert!(!NumFmt::general().is_date());
    assert!(NumFmt::general().is_general());
    assert!(NumFmt::parse("GENERAL").is_general());
    assert!(!NumFmt::parse("0").is_general());
    assert!(!NumFmt::parse("\"x\"General").is_general());
    assert!(NumFmt::general().is_valid());
}

fn text_of(value: Value<'_>, code: &str) -> Result<String, ()> {
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: false,
    };
    text_function(value, code, &options)
}

#[test]
fn the_text_function() {
    let number = Value::Number(1234.5678);
    assert_eq!(text_of(number, "0.00"), Ok("1234.57".to_string()));
    assert_eq!(text_of(number, "#,##0"), Ok("1,235".to_string()));
    assert_eq!(
        text_of(Value::Number(45292.75), "yyyy-mm-dd"),
        Ok("2024-01-01".to_string())
    );
    assert_eq!(
        text_of(Value::Text("abc"), "\"a\"@"),
        Ok("aabc".to_string())
    );
    assert_eq!(text_of(Value::Bool(true), "0"), Ok("TRUE".to_string()));
    // Fill leaves nothing behind in plain text.
    assert_eq!(text_of(number, "*x0"), Ok("1235".to_string()));
    // A code Excel can't read, and a date it can't show, are both #VALUE!.
    assert_eq!(text_of(number, "\"unterminated"), Err(()));
    assert_eq!(text_of(number, "[Foo]0"), Err(()));
    assert_eq!(text_of(Value::Number(-1.0), "yyyy"), Err(()));
}

fn edit(value: Value<'_>, format: &NumFmt) -> String {
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: false,
    };
    let mut out = String::new();
    edit_text(value, format, &options, &mut out);
    out
}

#[test]
fn the_formula_bar_shows_numbers_at_full_precision() {
    let general = NumFmt::general();
    let shown = |value: f64| edit(Value::Number(value), &general);
    assert_eq!(shown(0.0), "0");
    assert_eq!(shown(1234.5678), "1234.5678");
    assert_eq!(shown(0.1 + 0.2), "0.3");
    assert_eq!(shown(-1234.5678), "-1234.5678");
    assert_eq!(shown(1e15), "1000000000000000");
    assert_eq!(shown(1234567890123456.0), "1234567890123460");
    assert_eq!(shown(1.23456789012345e20), "123456789012345000000");
    assert_eq!(shown(1e20), "100000000000000000000");
    assert_eq!(shown(0.00001234), "0.00001234");
    assert_eq!(shown(1e-10), "0.0000000001");
    assert_eq!(shown(-1e-10), "-0.0000000001");
    // Past 21 characters it turns scientific.
    assert_eq!(shown(1e21), "1E+21");
    assert_eq!(shown(1e-20), "1E-20");
    assert_eq!(shown(1.5e-19), "1.5E-19");
    assert_eq!(shown(1.23456789012345e-10), "1.23456789012345E-10");
    assert_eq!(shown(1.7976931348623157e308), "1.79769313486232E+308");
}

#[test]
fn the_formula_bar_keeps_percents_and_dates_readable() {
    let percent = NumFmt::parse("0%");
    assert_eq!(edit(Value::Number(0.125), &percent), "12.5%");
    assert_eq!(edit(Value::Number(1.5), &NumFmt::parse("0.00%")), "150%");
    assert_eq!(edit(Value::Number(-0.5), &percent), "-50%");

    let date = NumFmt::parse("yyyy-mm-dd");
    assert_eq!(edit(Value::Number(45292.0), &date), "1/1/2024");
    assert_eq!(edit(Value::Number(45292.5), &date), "1/1/2024 12:00:00 PM");
    assert_eq!(edit(Value::Number(0.5), &date), "1/0/1900 12:00:00 PM");
    assert_eq!(edit(Value::Number(0.0), &date), "1/0/1900");

    let time = NumFmt::parse("h:mm");
    assert_eq!(edit(Value::Number(0.75), &time), "6:00:00 PM");
    assert_eq!(edit(Value::Number(0.0), &time), "12:00:00 AM");
    assert_eq!(edit(Value::Number(1.5), &time), "1/1/1900 12:00:00 PM");
    assert_eq!(
        edit(Value::Number(1.5), &NumFmt::parse("[h]:mm")),
        "1/1/1900 12:00:00 PM"
    );

    // A serial with no date is only a number.
    assert_eq!(edit(Value::Number(-1.0), &date), "-1");
    assert_eq!(edit(Value::Number(2958466.0), &date), "2958466");
}

#[test]
fn the_formula_bar_shows_everything_else_as_it_is() {
    let format = NumFmt::parse("0.00");
    assert_eq!(edit(Value::Text("abc"), &format), "abc");
    assert_eq!(edit(Value::Bool(true), &format), "TRUE");
    assert_eq!(edit(Value::Bool(false), &format), "FALSE");
    assert_eq!(edit(Value::Error("#DIV/0!"), &format), "#DIV/0!");
    assert_eq!(edit(Value::Number(1.5), &format), "1.5");
}

fn parts(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
) -> Option<DateTimeParts> {
    Some(DateTimeParts {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
    })
}

#[test]
fn serials_to_parts_in_both_date_systems() {
    assert_eq!(
        serial_to_datetime(0.0, false),
        parts(1900, 1, 0, 0, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(1.0, false),
        parts(1900, 1, 1, 0, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(59.0, false),
        parts(1900, 2, 28, 0, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(60.0, false),
        parts(1900, 2, 29, 0, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(61.0, false),
        parts(1900, 3, 1, 0, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(45352.75, false),
        parts(2024, 3, 1, 18, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(45000.999999, false),
        parts(2023, 3, 15, 23, 59, 59, 914)
    );
    assert_eq!(
        serial_to_datetime(2958465.0, false),
        parts(9999, 12, 31, 0, 0, 0, 0)
    );
    assert_eq!(serial_to_datetime(0.0, true), parts(1904, 1, 1, 0, 0, 0, 0));
    assert_eq!(
        serial_to_datetime(60.0, true),
        parts(1904, 3, 1, 0, 0, 0, 0)
    );
    assert_eq!(
        serial_to_datetime(2957003.0, true),
        parts(9999, 12, 31, 0, 0, 0, 0)
    );
}

#[test]
fn serials_outside_the_calendar_have_no_parts() {
    assert_eq!(serial_to_datetime(-1.0, false), None);
    assert_eq!(serial_to_datetime(2958466.0, false), None);
    assert_eq!(serial_to_datetime(2957004.0, true), None);
    assert_eq!(serial_to_datetime(f64::NAN, false), None);
    assert_eq!(serial_to_datetime(f64::INFINITY, false), None);
}

#[test]
fn a_number_that_isnt_one_shows_as_hashes() {
    let (out, display) = with(&NumFmt::general(), Value::Number(f64::NAN));
    assert!(display.hashes);
    assert_eq!(out, "");
    let (_, display) = with(&NumFmt::parse("0"), Value::Number(f64::INFINITY));
    assert!(display.hashes);
}

#[test]
fn no_code_or_value_can_make_it_panic() {
    let pieces = [
        "0",
        "#",
        "?",
        ".",
        ",",
        "%",
        "/",
        ":",
        ";",
        "@",
        "E+",
        "E-",
        "e",
        "E",
        "General",
        "y",
        "yyyy",
        "m",
        "mmm",
        "d",
        "dddd",
        "h",
        "s",
        "ss",
        ".0",
        ".000",
        "AM/PM",
        "A/P",
        "b",
        "b2",
        "g",
        "\"",
        "\"a\"",
        "\\",
        "\\x",
        "_",
        "_)",
        "*",
        "*-",
        "[",
        "]",
        "[h]",
        "[m]",
        "[s]",
        "[Red]",
        "[Color9]",
        "[>1]",
        "[<=-5]",
        "[=0]",
        "[$-401]",
        "[$-60401]",
        "[$-2000401]",
        "[$-F800]",
        "[$-F400]",
        "[$€-407]",
        "[$-411]",
        "[$-30409]",
        "[$-70409]",
        "[DBNum1]",
        "1",
        "9",
        "16",
        " ",
        "-",
        "$",
        "€",
        "٠",
        "x",
        "n",
    ];
    let values = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        60.0,
        45292.75,
        -45292.75,
        2958465.999999,
        1e15,
        1e100,
        -1e100,
        1.7976931348623157e308,
        -1.7976931348623157e308,
        5e-324,
        f64::NAN,
        f64::INFINITY,
    ];
    let locale = Locale::en_us();
    let plain = Options {
        locale: &locale,
        date1904: false,
    };
    let other = Options {
        locale: &locale,
        date1904: true,
    };
    // A small fixed generator, so a failure repeats.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = |limit: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % limit as u64) as usize
    };
    for _ in 0..20_000 {
        let mut code = String::new();
        for _ in 0..next(8) {
            code.push_str(pieces[next(pieces.len())]);
        }
        let format = NumFmt::parse(&code);
        let mut out = String::new();
        for value in values {
            out.clear();
            let display = format.format(Value::Number(value), &plain, &mut out);
            if let Some((_, at)) = display.fill {
                assert!(out.is_char_boundary(at), "{code}");
            }
            out.clear();
            format.format(Value::Number(value), &other, &mut out);
            out.clear();
            edit_text(Value::Number(value), &format, &plain, &mut out);
        }
        out.clear();
        format.format(Value::Text("abc"), &plain, &mut out);
        out.clear();
        format.format(Value::Bool(true), &plain, &mut out);
    }
}

#[test]
fn text_takes_a_lowercase_exponent_mark_unless_it_starts_a_date() {
    let locale = Locale::en_us();
    let options = Options {
        locale: &locale,
        date1904: false,
    };
    let text = |value: f64, code: &str| text_function(Value::Number(value), code, &options);
    assert_eq!(text(12345.0, "0.00e+00").as_deref(), Ok("1.23e+04"));
    assert_eq!(text(1.2e142, "|#.#|e+|000|").as_deref(), Ok("|1.2|e|+142|"));
    assert_eq!(text(45306.0, "e-mm-dd").as_deref(), Ok("2024-01-15"));
}
