//! 簡単なasync/awaitのサンプルプログラム
//! このプログラムをkokiaでデバッグすることで、GenFuture::pollの監視と
//! 論理スタック（awaitチェーン）の可視化をテストします。

use std::time::Duration;

/// 非同期関数1: 数値を2倍にする
async fn double(x: i32) -> i32 {
    println!("double({}) called", x);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let result = x * 2;
    println!("double({}) = {}", x, result);
    result
}

/// 非同期関数2: 2つの値を加算する
async fn add(a: i32, b: i32) -> i32 {
    println!("add({}, {}) called", a, b);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let result = a + b;
    println!("add({}, {}) = {}", a, b, result);
    result
}

/// 非同期関数3: 複数の非同期関数を組み合わせる
async fn compute(x: i32, y: i32) -> i32 {
    println!("compute({}, {}) called", x, y);

    // xを2倍にする
    let doubled_x = double(x).await;

    // yを2倍にする
    let doubled_y = double(y).await;

    // 結果を加算する
    let sum = add(doubled_x, doubled_y).await;

    println!("compute({}, {}) = {}", x, y, sum);
    sum
}

async fn heavy() {
    println!("heavy() called");
    for i in 0..10000 {
        let _ = i * i;
    }   
    println!("heavy() completed");
}

async fn breakpoint() {
    println!("breakpoint() called");
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("breakpoint() completed");
}

/// 変数表示テスト用の関数（同期版）
fn test_variables_sync() {
    println!("test_variables_sync() called");

    let message = String::from("Hello, Kokia!");
    let numbers = vec![1, 2, 3, 4, 5];
    let maybe_value = Some(42);
    let result_value: Result<i32, String> = Ok(100);

    println!("message: {}", message);
    println!("numbers: {:?}", numbers);
    println!("maybe_value: {:?}", maybe_value);
    println!("result_value: {:?}", result_value);

    println!("test_variables_sync() completed");
}

/// メイン関数
#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("=== Kokia Simple Async Example ===");
    println!("This program demonstrates async/await for debugging with kokia.");
    println!();

    // Test variable display
    test_variables_sync();

    let result = compute(5, 10).await;

    let mut jhs = Vec::new();
    for _ in 0..4 {
        let jh = tokio::spawn(async {
            heavy().await;
        });
        jhs.push(jh);
    }
    breakpoint().await;
    for jh in jhs {
        let _ = jh.await;
    }
    println!("Final result: {}", result);
    println!("Expected: (5*2) + (10*2) = 10 + 20 = 30");
}
