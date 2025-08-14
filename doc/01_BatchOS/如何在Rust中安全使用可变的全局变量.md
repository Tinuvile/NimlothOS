对于`AppManager`，它需要能够被任何函数访问，并且`current_app`字段需要在运行时修改。

如果使用`static mut`，则必须用`unsafe`进行包裹，但尽量避免使用`unsafe`，这样才能让编译器负责更多的安全性检查。

单独使用`static`缺少可变性，可以用Rust内置的数据结构将借用检查推迟到运行时，即用`RefCell`来包裹`AppManager`，然后调用`borrow`和`borrow_mut`便可发起借用并获得一个对值的不可变/可变借用的标志，可以像引用一样使用。但这样会遇到`Sync`问题。

解决方案是在`RefCell`的基础上再封装一个`UPSafeCell`，允许我们在单核上安全使用可变全局变量。要访问数据时，先调用`exclusive_access`获得数据的可变借用标记，通过它完成数据的读写，操作完成后再销毁这个标记。