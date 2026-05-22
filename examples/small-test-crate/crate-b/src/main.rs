use crate_a::used_function;

fn main() {
    let s = used_function();
    println!("{}", s.field);
}
