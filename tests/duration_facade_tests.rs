use rhp::{run_source, PhpValue};
use rhp::vm::facade::clear_runtime_closure_macros;

fn strings(res: PhpValue) -> Vec<String> {
    match res {
        PhpValue::Array(items) | PhpValue::List(items) => {
            items.borrow().iter().map(|v| v.to_php_string()).collect()
        }
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn test_duration_factories_spec_and_humans() {
    clear_runtime_closure_macros();
    let code = r#"
        $h = Duration::hours(2)->minutes(30);
        $iso = Duration::fromString("P1Y2M3DT4H5M6S")->spec();
        $words = Duration::fromString("2 hours 30 minutes")->forHumans();
        $alias = CarbonInterval::minutes(90)->cascade()->spec();
        $created = Duration::create(1, 2, 0, 3, 4, 5, 6)->spec();
        $neg = Duration::hours(-5)->spec();
        $days = Duration::days(45)->forHumans();
        $short = Duration::hours(2)->minutes(5)->forHumans(["short" => true]);
        $parts = Duration::days(45)->forHumans(["parts" => 1]);
        $ago = Duration::days(3)->invert()->forHumans(["syntax" => Duration::DIFF_RELATIVE_TO_NOW]);
        return [
            $h->spec(),
            $h->forHumans(),
            $h->hours,
            $h->minutes,
            $iso,
            $words,
            $alias,
            $created,
            $neg,
            $days,
            $short,
            $parts,
            $ago,
            Duration::HOURS_PER_DAY,
            Duration::WEEKS_PER_MONTH,
        ];
    "#;
    let got = strings(run_source(code).expect("duration factories"));
    assert_eq!(got[0], "PT2H30M");
    assert_eq!(got[1], "2 hours 30 minutes");
    assert_eq!(got[2], "2");
    assert_eq!(got[3], "30");
    assert_eq!(got[4], "P1Y2M3DT4H5M6S");
    assert_eq!(got[5], "2 hours 30 minutes");
    assert_eq!(got[6], "PT1H30M");
    assert_eq!(got[7], "P1Y2M3DT4H5M6S");
    assert_eq!(got[8], "-PT5H");
    assert_eq!(got[9], "1 month 2 weeks 3 days");
    assert_eq!(got[10], "2h 5m");
    assert_eq!(got[11], "1 month");
    assert_eq!(got[12], "3 days ago");
    assert_eq!(got[13], "24");
    assert_eq!(got[14], "4");
}

#[test]
fn test_duration_arithmetic_compare_and_total() {
    clear_runtime_closure_macros();
    let code = r#"
        $sum = Duration::hours(2)->add(Duration::minutes(30))->spec();
        $sub = Duration::hours(2)->sub(Duration::minutes(30))->spec();
        $half = Duration::hours(1)->divide(2)->forHumans();
        $double = Duration::minutes(30)->times(2)->spec();
        $eq = Duration::hours(1)->eq(Duration::minutes(60));
        $gt = Duration::hours(3)->gt(Duration::hours(2));
        $total_hours = Duration::minutes(90)->total("hours");
        $total_days = Duration::months(1)->totalDays();
        $fmt = Duration::create(1, 2, 0, 3, 4, 5, 6)->format("%y years %m months");
        $between = Duration::hours(2)->between(Duration::hours(1), Duration::hours(3));
        $empty = Duration::seconds(0)->isEmpty();
        return [$sum, $sub, $half, $double, $eq, $gt, $total_hours, $total_days, $fmt, $between, $empty];
    "#;
    let res = run_source(code).expect("duration arithmetic");
    let (PhpValue::Array(items) | PhpValue::List(items)) = res else {
        panic!("expected array, got {:?}", res);
    };
    let items = items.borrow();
    assert_eq!(items[0].to_php_string(), "PT2H30M");
    assert_eq!(items[1].to_php_string(), "PT1H30M");
    assert_eq!(items[2].to_php_string(), "30 minutes");
    assert_eq!(items[3].to_php_string(), "PT1H");
    assert_eq!(items[4].to_php_string(), "1");
    assert_eq!(items[5].to_php_string(), "1");
    match &items[6] {
        PhpValue::Float(f) => assert!((f - 1.5).abs() < 1e-9, "total hours {f}"),
        other => panic!("expected float, got {other:?}"),
    }
    match &items[7] {
        PhpValue::Float(f) => assert!((f - 28.0).abs() < 1e-9, "total days {f}"),
        other => panic!("expected float, got {other:?}"),
    }
    assert_eq!(items[8].to_php_string(), "1 years 2 months");
    assert_eq!(items[9].to_php_string(), "1");
    assert_eq!(items[10].to_php_string(), "1");
}

#[test]
fn test_duration_with_time() {
    clear_runtime_closure_macros();
    let code = r#"
        $t = Time::parse("2024-01-01 00:00:00");
        $t->add(Duration::days(2)->hours(3));
        $added = $t->toDateTimeString();
        $a = Time::parse("2024-01-01 00:00:00");
        $b = Time::parse("2024-03-01 00:00:00");
        $diff = $a->diff($b);
        $back = Time::parse("2024-03-01 00:00:00")->sub(Duration::months(2))->toDateString();
        return [$added, $diff->spec(), $diff->forHumans(), $back, $diff instanceof Duration];
    "#;
    let got = strings(run_source(code).expect("duration with time"));
    assert_eq!(got[0], "2024-01-03 03:00:00");
    assert_eq!(got[1], "P2M");
    assert_eq!(got[2], "2 months");
    assert_eq!(got[3], "2024-01-01");
    assert_eq!(got[4], "1");
}
