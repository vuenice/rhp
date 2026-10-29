use rhp::{run_source, PhpValue};

#[test]
fn test_foreach_custom_iterator_aggregate() {
    let source = r#"
        class TeamList implements IteratorAggregate {
            private array $members;

            public function __construct(array $members) {
                $this->members = $members;
            }

            public function getIterator(): Traversable {
                return new ArrayIterator($this->members);
            }
        }

        $team = new TeamList(["Alice", "Bob", "Charlie"]);
        $collected = [];
        foreach ($team as $key => $member) {
            $collected[] = $key . ":" . $member;
        }
        return $collected;
    "#;

    let result = run_source(source).expect("Custom IteratorAggregate should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("0:Alice"),
            PhpValue::string("1:Bob"),
            PhpValue::string("2:Charlie"),
        ])
    );
}

#[test]
fn test_foreach_custom_iterator_step_by_step() {
    let source = r#"
        class RangeIterator implements Iterator {
            private int $start;
            private int $end;
            private int $curr;

            public function __construct(int $start, int $end) {
                $this->start = $start;
                $this->end = $end;
                $this->curr = $start;
            }

            public function rewind(): void {
                $this->curr = $this->start;
            }

            public function valid(): bool {
                return $this->curr <= $this->end;
            }

            public function current(): mixed {
                return $this->curr * 10;
            }

            public function key(): mixed {
                return $this->curr - $this->start;
            }

            public function next(): void {
                $this->curr += 1;
            }
        }

        $range = new RangeIterator(1, 4);
        $keys = [];
        $values = [];
        foreach ($range as $k => $v) {
            $keys[] = $k;
            $values[] = $v;
        }
        return ["keys" => $keys, "values" => $values];
    "#;

    let result = run_source(source).expect("Custom Iterator should execute");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            (
                "keys".to_string(),
                PhpValue::list(vec![
                    PhpValue::Int(0),
                    PhpValue::Int(1),
                    PhpValue::Int(2),
                    PhpValue::Int(3),
                ])
            ),
            (
                "values".to_string(),
                PhpValue::list(vec![
                    PhpValue::Int(10),
                    PhpValue::Int(20),
                    PhpValue::Int(30),
                    PhpValue::Int(40),
                ])
            ),
        ])
    );
}

#[test]
fn test_foreach_plain_object_visible_properties() {
    let source = r#"
        class UserDTO {
            public string $name = "Yogesh";
            public string $role = "Engineer";
            public int $level = 7;
        }

        $dto = new UserDTO();
        $props = [];
        foreach ($dto as $key => $val) {
            $props[] = $key . "=" . $val;
        }
        return $props;
    "#;

    let result = run_source(source).expect("Plain object property iteration should execute");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::string("name=Yogesh"),
            PhpValue::string("role=Engineer"),
            PhpValue::string("level=7"),
        ])
    );
}

#[test]
fn test_foreach_nested_loops_and_matrices() {
    let source = r#"
        class MatrixRow implements IteratorAggregate {
            private array $cells;
            public function __construct(array $cells) {
                $this->cells = $cells;
            }
            public function getIterator(): Traversable {
                return new ArrayIterator($this->cells);
            }
        }

        class MatrixGrid implements IteratorAggregate {
            private array $rows;
            public function __construct(array $rows) {
                $this->rows = $rows;
            }
            public function getIterator(): Traversable {
                return new ArrayIterator($this->rows);
            }
        }

        $grid = new MatrixGrid([
            new MatrixRow([1, 2]),
            new MatrixRow([3, 4]),
            new MatrixRow([5, 6]),
        ]);

        $sum = 0;
        foreach ($grid as $row) {
            foreach ($row as $val) {
                $sum += $val;
            }
        }
        return $sum;
    "#;

    let result = run_source(source).expect("Nested object iteration should succeed");
    assert_eq!(result, PhpValue::Int(21));
}

#[test]
fn test_foreach_break_and_continue_on_object() {
    let source = r#"
        class NumberBox implements IteratorAggregate {
            private array $numbers;
            public function __construct(array $nums) {
                $this->numbers = $nums;
            }
            public function getIterator(): Traversable {
                return new ArrayIterator($this->numbers);
            }
        }

        $box = new NumberBox([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        $collected = [];
        foreach ($box as $num) {
            if ($num % 2 == 0) {
                continue;
            }
            if ($num > 7) {
                break;
            }
            $collected[] = $num;
        }
        return $collected;
    "#;

    let result = run_source(source).expect("Break and continue on object foreach should succeed");
    assert_eq!(
        result,
        PhpValue::list(vec![
            PhpValue::Int(1),
            PhpValue::Int(3),
            PhpValue::Int(5),
            PhpValue::Int(7),
        ])
    );
}

#[test]
fn test_foreach_laravel_collection_and_collect() {
    let source = r#"
        $c1 = collect(["alpha", "beta", "gamma"]);
        $res1 = [];
        foreach ($c1 as $k => $v) {
            $res1[] = $k . ":" . $v;
        }

        $c2 = new Collection(["x" => 10, "y" => 20]);
        $res2 = 0;
        foreach ($c2 as $val) {
            $res2 += $val;
        }

        return ["list" => $res1, "sum" => $res2];
    "#;

    let result = run_source(source).expect("Laravel Collection iteration should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            (
                "list".to_string(),
                PhpValue::list(vec![
                    PhpValue::string("0:alpha"),
                    PhpValue::string("1:beta"),
                    PhpValue::string("2:gamma"),
                ])
            ),
            ("sum".to_string(), PhpValue::Int(30)),
        ])
    );
}

#[test]
fn test_foreach_laravel_paginator() {
    let source = r#"
        $paginator = new LengthAwarePaginator(["order_1", "order_2", "order_3"], 50, 15, 1);
        $orders = [];
        foreach ($paginator as $order) {
            $orders[] = $order;
        }
        return [
            "orders" => $orders,
            "total" => $paginator->total(),
            "perPage" => $paginator->perPage(),
            "currentPage" => $paginator->currentPage(),
        ];
    "#;

    let result = run_source(source).expect("LengthAwarePaginator iteration should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            (
                "orders".to_string(),
                PhpValue::list(vec![
                    PhpValue::string("order_1"),
                    PhpValue::string("order_2"),
                    PhpValue::string("order_3"),
                ])
            ),
            ("total".to_string(), PhpValue::Int(50)),
            ("perPage".to_string(), PhpValue::Int(15)),
            ("currentPage".to_string(), PhpValue::Int(1)),
        ])
    );
}

#[test]
fn test_foreach_laravel_db_query_and_paginate() {
    let source = r#"
        $users = DB::table("users")->get();
        $userCount = 0;
        foreach ($users as $user) {
            $userCount += $user->id;
        }

        $paginated = DB::table("users")->paginate(5);
        $paginatedCount = 0;
        foreach ($paginated as $pUser) {
            $paginatedCount += $pUser->id;
        }

        return [
            "getSum" => $userCount,
            "paginateSum" => $paginatedCount,
            "total" => $paginated->total(),
            "perPage" => $paginated->perPage(),
        ];
    "#;

    let result = run_source(source).expect("Laravel DB query and paginate iteration should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            ("getSum".to_string(), PhpValue::Int(1)),
            ("paginateSum".to_string(), PhpValue::Int(1)),
            ("total".to_string(), PhpValue::Int(1)),
            ("perPage".to_string(), PhpValue::Int(5)),
        ])
    );
}

#[test]
fn test_instanceof_and_type_contracts() {
    let source = r#"
        $arrIt = new ArrayIterator([1, 2, 3]);
        $col = new Collection(["a", "b"]);
        $paginator = new LengthAwarePaginator(["x"], 10, 5, 1);

        $t1 = $arrIt instanceof Traversable;
        $t2 = $arrIt instanceof Iterator;
        $t3 = $col instanceof Traversable;
        $t4 = $col instanceof IteratorAggregate;
        $t5 = $paginator instanceof Traversable;
        $t6 = $paginator instanceof IteratorAggregate;

        return [
            "arrIt_trav" => $t1,
            "arrIt_iter" => $t2,
            "col_trav" => $t3,
            "col_agg" => $t4,
            "pag_trav" => $t5,
            "pag_agg" => $t6,
        ];
    "#;

    let result = run_source(source).expect("Instanceof contracts should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            ("arrIt_trav".to_string(), PhpValue::Bool(true)),
            ("arrIt_iter".to_string(), PhpValue::Bool(true)),
            ("col_trav".to_string(), PhpValue::Bool(true)),
            ("col_agg".to_string(), PhpValue::Bool(true)),
            ("pag_trav".to_string(), PhpValue::Bool(true)),
            ("pag_agg".to_string(), PhpValue::Bool(true)),
        ])
    );
}

#[test]
fn test_iterable_parameter_type_hint_contract() {
    let source = r#"
        function sumIterable(iterable $items): int {
            $total = 0;
            foreach ($items as $val) {
                $total += $val;
            }
            return $total;
        }

        $s1 = sumIterable([10, 20, 30]);
        $s2 = sumIterable(new ArrayIterator([5, 15]));
        $s3 = sumIterable(new Collection([100, 200]));

        return $s1 + $s2 + $s3;
    "#;

    let result = run_source(source).expect("Iterable type hint contract should succeed");
    assert_eq!(result, PhpValue::Int(380));
}

#[test]
fn test_builtin_array_iterator_direct() {
    let source = r#"
        $it = new ArrayIterator(["first" => 100, "second" => 200, "third" => 300]);
        $keys = [];
        $values = [];
        foreach ($it as $k => $v) {
            $keys[] = $k;
            $values[] = $v;
        }
        return [
            "keys" => $keys,
            "values" => $values,
            "count" => $it->count(),
        ];
    "#;

    let result = run_source(source).expect("Direct ArrayIterator foreach should succeed");
    assert_eq!(
        result,
        PhpValue::dict(vec![
            (
                "keys".to_string(),
                PhpValue::list(vec![
                    PhpValue::string("first"),
                    PhpValue::string("second"),
                    PhpValue::string("third"),
                ])
            ),
            (
                "values".to_string(),
                PhpValue::list(vec![
                    PhpValue::Int(100),
                    PhpValue::Int(200),
                    PhpValue::Int(300),
                ])
            ),
            ("count".to_string(), PhpValue::Int(3)),
        ])
    );
}
