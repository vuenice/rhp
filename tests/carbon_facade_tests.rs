use rhp::run_source;
use rhp::vm::carbon::CarbonDate;
use rhp::vm::facade::clear_runtime_closure_macros;
use rhp::vm::value::PhpValue;

#[test]
fn test_carbon_create_parse_and_formatting() {
    clear_runtime_closure_macros();
    CarbonDate::clear_test_now();

    let code = r#"
        $d1 = Carbon::create(2024, 5, 20, 14, 30, 45);
        $s_datetime = $d1->toDateTimeString();
        $s_date = $d1->toDateString();
        $s_dat = $d1->toDatString();
        $s_time = $d1->toTimeString();
        $s_formatted = $d1->toFormattedDateString();
        $s_custom = $d1->format("Y/m/d H:i");

        $parsed = Carbon::parse("2024-12-25 08:00:00");
        $parsed_str = $parsed->toDateTimeString();

        return [
            $s_datetime,
            $s_date,
            $s_dat,
            $s_time,
            $s_formatted,
            $s_custom,
            $parsed_str,
            $d1->year,
            $d1->month,
            $d1->day,
            $d1->hour,
            $d1->minute,
            $d1->second,
        ];
    "#;

    let res = run_source(code).expect("Script execution failed");
    if let PhpValue::Array(items_rc) = res {
        let items = items_rc.borrow();
        assert_eq!(items[0].to_php_string(), "2024-05-20 14:30:45");
        assert_eq!(items[1].to_php_string(), "2024-05-20");
        assert_eq!(items[2].to_php_string(), "2024-05-20");
        assert_eq!(items[3].to_php_string(), "14:30:45");
        assert_eq!(items[4].to_php_string(), "May 20, 2024");
        assert_eq!(items[5].to_php_string(), "2024/05/20 14:30");
        assert_eq!(items[6].to_php_string(), "2024-12-25 08:00:00");
        assert_eq!(items[7].to_i64(), Some(2024));
        assert_eq!(items[8].to_i64(), Some(5));
        assert_eq!(items[9].to_i64(), Some(20));
        assert_eq!(items[10].to_i64(), Some(14));
        assert_eq!(items[11].to_i64(), Some(30));
        assert_eq!(items[12].to_i64(), Some(45));
    } else {
        panic!("Expected array return, got: {:?}", res);
    }
}

#[test]
fn test_carbon_fluent_add_and_sub() {
    clear_runtime_closure_macros();
    CarbonDate::clear_test_now();

    let code = r#"
        $d = Carbon::parse("2024-01-15 10:00:00");
        
        $d->addDay();
        $after_add_day = $d->toDateString();

        $d->addDays(5);
        $after_add_days = $d->toDateString();

        $d->subDay();
        $after_sub_day = $d->toDateString();

        $d->subDays(3);
        $after_sub_days = $d->toDateString();

        $d->addWeek();
        $after_add_week = $d->toDateString();

        $d->addMonth();
        $after_add_month = $d->toDateString();

        $d->addYear();
        $after_add_year = $d->toDateString();

        // Chaining
        $chained = Carbon::parse("2024-06-01 12:00:00")->addDays(4)->subHours(2)->toDateTimeString();

        return [
            $after_add_day,
            $after_add_days,
            $after_sub_day,
            $after_sub_days,
            $after_add_week,
            $after_add_month,
            $after_add_year,
            $chained,
        ];
    "#;

    let res = run_source(code).expect("Script execution failed");
    if let PhpValue::Array(items_rc) = res {
        let items = items_rc.borrow();
        assert_eq!(items[0].to_php_string(), "2024-01-16");
        assert_eq!(items[1].to_php_string(), "2024-01-21");
        assert_eq!(items[2].to_php_string(), "2024-01-20");
        assert_eq!(items[3].to_php_string(), "2024-01-17");
        assert_eq!(items[4].to_php_string(), "2024-01-24");
        assert_eq!(items[5].to_php_string(), "2024-02-24");
        assert_eq!(items[6].to_php_string(), "2025-02-24");
        assert_eq!(items[7].to_php_string(), "2024-06-05 10:00:00");
    } else {
        panic!("Expected array return, got: {:?}", res);
    }
}

#[test]
fn test_carbon_last_of_month_and_boundaries() {
    clear_runtime_closure_macros();
    CarbonDate::clear_test_now();

    let code = r#"
        // Leap year February
        $feb = Carbon::parse("2024-02-10 15:30:00");
        $feb_last = $feb->lastOfMonth()->toDateTimeString();

        // Non-leap year February
        $feb2023 = Carbon::parse("2023-02-05 10:00:00");
        $feb2023_last = $feb2023->lastOfMonth()->toDateTimeString();

        // January
        $jan = Carbon::parse("2024-01-15 00:00:00");
        $jan_first = $jan->startOfMonth()->toDateTimeString();
        $jan_last = $jan->endOfMonth()->toDateTimeString();

        // Start / End of day
        $day = Carbon::parse("2024-07-20 16:45:12");
        $start_day = $day->startOfDay()->toDateTimeString();
        $end_day = $day->endOfDay()->toDateTimeString();

        // Start / End of year
        $year = Carbon::parse("2024-07-20 16:45:12");
        $start_year = $year->startOfYear()->toDateTimeString();
        $end_year = $year->endOfYear()->toDateTimeString();

        return [
            $feb_last,
            $feb2023_last,
            $jan_first,
            $jan_last,
            $start_day,
            $end_day,
            $start_year,
            $end_year,
        ];
    "#;

    let res = run_source(code).expect("Script execution failed");
    if let PhpValue::Array(items_rc) = res {
        let items = items_rc.borrow();
        assert_eq!(items[0].to_php_string(), "2024-02-29 23:59:59");
        assert_eq!(items[1].to_php_string(), "2023-02-28 23:59:59");
        assert_eq!(items[2].to_php_string(), "2024-01-01 00:00:00");
        assert_eq!(items[3].to_php_string(), "2024-01-31 23:59:59");
        assert_eq!(items[4].to_php_string(), "2024-07-20 00:00:00");
        assert_eq!(items[5].to_php_string(), "2024-07-20 23:59:59");
        assert_eq!(items[6].to_php_string(), "2024-01-01 00:00:00");
        assert_eq!(items[7].to_php_string(), "2024-12-31 23:59:59");
    } else {
        panic!("Expected array return, got: {:?}", res);
    }
}

#[test]
fn test_carbon_static_direct_calls() {
    clear_runtime_closure_macros();
    CarbonDate::set_test_now(CarbonDate::parse("2024-08-15 11:22:33", None).unwrap());

    let code = r#"
        $datetime = Carbon::toDateTimeString();
        $date = Carbon::toDateString();
        $dat = Carbon::toDatString();
        $time = Carbon::toTimeString();
        $tomorrow = Carbon::addDay()->toDateString();
        $last_month = Carbon::lastOfMonth()->toDateTimeString();

        return [
            $datetime,
            $date,
            $dat,
            $time,
            $tomorrow,
            $last_month,
        ];
    "#;

    let res = run_source(code).expect("Script execution failed");
    CarbonDate::clear_test_now();

    if let PhpValue::Array(items_rc) = res {
        let items = items_rc.borrow();
        assert_eq!(items[0].to_php_string(), "2024-08-15 11:22:33");
        assert_eq!(items[1].to_php_string(), "2024-08-15");
        assert_eq!(items[2].to_php_string(), "2024-08-15");
        assert_eq!(items[3].to_php_string(), "11:22:33");
        assert_eq!(items[4].to_php_string(), "2024-08-16");
        assert_eq!(items[5].to_php_string(), "2024-08-31 23:59:59");
    } else {
        panic!("Expected array return, got: {:?}", res);
    }
}

#[test]
fn test_carbon_comparisons_and_differences() {
    clear_runtime_closure_macros();
    CarbonDate::set_test_now(CarbonDate::parse("2024-05-15 12:00:00", None).unwrap());

    let code = r#"
        $now = Carbon::now();
        $past = Carbon::parse("2024-05-10 12:00:00");
        $future = Carbon::parse("2024-05-20 12:00:00");

        $is_past = $past->isPast();
        $is_future = $future->isFuture();
        $is_today = $now->isToday();
        $is_leap = Carbon::parse("2024-01-01")->isLeapYear();
        $is_not_leap = Carbon::parse("2023-01-01")->isLeapYear();

        $gt = $future->gt($past);
        $lt = $past->lt($future);
        $between = $now->between($past, $future);

        $diff_days = $future->diffInDays($past);
        $diff_hours = $future->diffInHours($past);

        $for_humans_past = $past->diffForHumans();
        $for_humans_future = $future->diffForHumans();

        return [
            $is_past,
            $is_future,
            $is_today,
            $is_leap,
            $is_not_leap,
            $gt,
            $lt,
            $between,
            $diff_days,
            $diff_hours,
            $for_humans_past,
            $for_humans_future,
        ];
    "#;

    let res = run_source(code).expect("Script execution failed");
    CarbonDate::clear_test_now();

    if let PhpValue::Array(items_rc) = res {
        let items = items_rc.borrow();
        assert_eq!(items[0].to_bool(), Some(true));
        assert_eq!(items[1].to_bool(), Some(true));
        assert_eq!(items[2].to_bool(), Some(true));
        assert_eq!(items[3].to_bool(), Some(true));
        assert_eq!(items[4].to_bool(), Some(false));
        assert_eq!(items[5].to_bool(), Some(true));
        assert_eq!(items[6].to_bool(), Some(true));
        assert_eq!(items[7].to_bool(), Some(true));
        assert_eq!(items[8].to_i64(), Some(10));
        assert_eq!(items[9].to_i64(), Some(240));
        assert_eq!(items[10].to_php_string(), "5 days ago");
        assert_eq!(items[11].to_php_string(), "in 5 days");
    } else {
        panic!("Expected array return, got: {:?}", res);
    }
}

#[test]
fn test_carbon_constants_and_echo_and_macros() {
    clear_runtime_closure_macros();
    CarbonDate::clear_test_now();

    let code = r#"
        $sun = Carbon::SUNDAY;
        $mon = Carbon::MONDAY;
        $days_in_week = Carbon::DAYS_PER_WEEK;
        $hours_in_day = Carbon::HOURS_PER_DAY;

        // String coercion
        $d = Carbon::parse("2024-03-15 09:30:00");
        $str = "Event on: " . $d;

        // Macro test
        Carbon::macro('toCustomTag', fn($obj) => "TAG:" . $obj->toDateString());
        $macro_res = Carbon::toCustomTag($d);

        return [
            $sun,
            $mon,
            $days_in_week,
            $hours_in_day,
            $str,
            $macro_res,
        ];
    "#;

    let res = run_source(code).expect("Script execution failed");
    if let PhpValue::Array(items_rc) = res {
        let items = items_rc.borrow();
        assert_eq!(items[0].to_i64(), Some(0));
        assert_eq!(items[1].to_i64(), Some(1));
        assert_eq!(items[2].to_i64(), Some(7));
        assert_eq!(items[3].to_i64(), Some(24));
        assert_eq!(items[4].to_php_string(), "Event on: 2024-03-15 09:30:00");
        assert_eq!(items[5].to_php_string(), "TAG:2024-03-15");
    } else {
        panic!("Expected array return, got: {:?}", res);
    }
}

#[test]
fn test_carbon_instantiation_forbidden() {
    clear_runtime_closure_macros();

    let code = r#"
        $c = new Carbon();
    "#;

    let res = run_source(code);
    assert!(
        res.is_err(),
        "Instantiating Carbon via `new Carbon()` should be forbidden by static facade guard"
    );
}
