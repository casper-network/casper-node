use wasmi::{FromRuntimeValue, RuntimeArgs, Trap};

pub(crate) trait Args
where
    Self: Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap>;
}

impl<T1> Args for (T1,)
where
    T1: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        Ok((a0,))
    }
}

impl<T1, T2> Args for (T1, T2)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        Ok((a0, a1))
    }
}

impl<T1, T2, T3> Args for (T1, T2, T3)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        Ok((a0, a1, a2))
    }
}

impl<T1, T2, T3, T4> Args for (T1, T2, T3, T4)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        Ok((a0, a1, a2, a3))
    }
}

impl<T1, T2, T3, T4, T5> Args for (T1, T2, T3, T4, T5)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        Ok((a0, a1, a2, a3, a4))
    }
}

impl<T1, T2, T3, T4, T5, T6> Args for (T1, T2, T3, T4, T5, T6)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
    T6: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        let a5: T6 = args.nth_checked(5)?;
        Ok((a0, a1, a2, a3, a4, a5))
    }
}

impl<T1, T2, T3, T4, T5, T6, T7> Args for (T1, T2, T3, T4, T5, T6, T7)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
    T6: FromRuntimeValue + Sized,
    T7: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        let a5: T6 = args.nth_checked(5)?;
        let a6: T7 = args.nth_checked(6)?;
        Ok((a0, a1, a2, a3, a4, a5, a6))
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8> Args for (T1, T2, T3, T4, T5, T6, T7, T8)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
    T6: FromRuntimeValue + Sized,
    T7: FromRuntimeValue + Sized,
    T8: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        let a5: T6 = args.nth_checked(5)?;
        let a6: T7 = args.nth_checked(6)?;
        let a7: T8 = args.nth_checked(7)?;
        Ok((a0, a1, a2, a3, a4, a5, a6, a7))
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9> Args for (T1, T2, T3, T4, T5, T6, T7, T8, T9)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
    T6: FromRuntimeValue + Sized,
    T7: FromRuntimeValue + Sized,
    T8: FromRuntimeValue + Sized,
    T9: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        let a5: T6 = args.nth_checked(5)?;
        let a6: T7 = args.nth_checked(6)?;
        let a7: T8 = args.nth_checked(7)?;
        let a8: T9 = args.nth_checked(8)?;
        Ok((a0, a1, a2, a3, a4, a5, a6, a7, a8))
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10> Args for (T1, T2, T3, T4, T5, T6, T7, T8, T9, T10)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
    T6: FromRuntimeValue + Sized,
    T7: FromRuntimeValue + Sized,
    T8: FromRuntimeValue + Sized,
    T9: FromRuntimeValue + Sized,
    T10: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        let a5: T6 = args.nth_checked(5)?;
        let a6: T7 = args.nth_checked(6)?;
        let a7: T8 = args.nth_checked(7)?;
        let a8: T9 = args.nth_checked(8)?;
        let a9: T10 = args.nth_checked(9)?;
        Ok((a0, a1, a2, a3, a4, a5, a6, a7, a8, a9))
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11> Args
    for (T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11)
where
    T1: FromRuntimeValue + Sized,
    T2: FromRuntimeValue + Sized,
    T3: FromRuntimeValue + Sized,
    T4: FromRuntimeValue + Sized,
    T5: FromRuntimeValue + Sized,
    T6: FromRuntimeValue + Sized,
    T7: FromRuntimeValue + Sized,
    T8: FromRuntimeValue + Sized,
    T9: FromRuntimeValue + Sized,
    T10: FromRuntimeValue + Sized,
    T11: FromRuntimeValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        let a2: T3 = args.nth_checked(2)?;
        let a3: T4 = args.nth_checked(3)?;
        let a4: T5 = args.nth_checked(4)?;
        let a5: T6 = args.nth_checked(5)?;
        let a6: T7 = args.nth_checked(6)?;
        let a7: T8 = args.nth_checked(7)?;
        let a8: T9 = args.nth_checked(8)?;
        let a9: T10 = args.nth_checked(9)?;
        let a10: T11 = args.nth_checked(10)?;
        Ok((a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10))
    }
}
