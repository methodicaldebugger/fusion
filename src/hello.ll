; Fusion LLVM IR

define i32 @main() {
entry:
  %local__Fx_0 = alloca i64, align 8
  store i64 10, ptr %local__Fx_0
  %local__Fy_0 = alloca i64, align 8
  store i64 32, ptr %local__Fy_0
  %t0 = load i64, ptr %local__Fx_0
  %t1 = load i64, ptr %local__Fy_0
  %t2 = add i64 %t0, %t1
  %t3 = call i32 (ptr, ...) @printf(ptr getelementptr inbounds ([5 x i8], ptr @.str0, i64 0, i64 0), i64 %t2)
  ret i32 0
}
@.str0 = private unnamed_addr constant [5 x i8] c"%ld\0A\00", align 1

declare i32 @printf(ptr, ...)
declare i32 @puts(ptr)
declare void @llvm.trap()

