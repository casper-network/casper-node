use casper_wasmi::{FromValue, RuntimeArgs, Trap};

pub(crate) trait Args
where
    Self: Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap>;
}

impl<T1> Args for (T1,)
where
    T1: FromValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        Ok((a0,))
    }
}

impl<T1, T2> Args for (T1, T2)
where
    T1: FromValue + Sized,
    T2: FromValue + Sized,
{
    fn parse(args: RuntimeArgs) -> Result<Self, Trap> {
        let a0: T1 = args.nth_checked(0)?;
        let a1: T2 = args.nth_checked(1)?;
        Ok((a0, a1))
    }
}

impl<T1, T2, T3> Args for (T1, T2, T3)
where
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
    T6: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
    T6: FromValue + Sized,
    T7: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
    T6: FromValue + Sized,
    T7: FromValue + Sized,
    T8: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
    T6: FromValue + Sized,
    T7: FromValue + Sized,
    T8: FromValue + Sized,
    T9: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
    T6: FromValue + Sized,
    T7: FromValue + Sized,
    T8: FromValue + Sized,
    T9: FromValue + Sized,
    T10: FromValue + Sized,
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
    T1: FromValue + Sized,
    T2: FromValue + Sized,
    T3: FromValue + Sized,
    T4: FromValue + Sized,
    T5: FromValue + Sized,
    T6: FromValue + Sized,
    T7: FromValue + Sized,
    T8: FromValue + Sized,
    T9: FromValue + Sized,
    T10: FromValue + Sized,
    T11: FromValue + Sized,
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
