namespace Probe;

public interface IWorker
{
    void /*def:interface*/Run();
    void /*def:default-interface*/Default() { }
}

public class Worker : IWorker
{
    public /*def:constructor*/Worker() { }
    public virtual void /*def:virtual*/Run() { }
    public int /*def:field*/Count;
    public int /*def:property*/Value { get; set; }
}

public class Derived : Worker
{
    public override void /*def:override*/Run() { }
}

public static class Targets
{
    public const int /*def:constant*/Limit = 5;
    public static void /*def:int*/Pick(int value) { }
    public static void /*def:string*/Pick(string value) { }
    public static T /*def:generic*/Identity<T>(T value) => value;
    public static void /*def:extension*/Touch(this Worker worker) { }
    public static void /*def:delegate-target*/Callback() { }
    public static void OnlyNamed() { }
}

public class Alpha
{
    public void /*def:alpha*/Act() { }
}

public class Beta
{
    public void /*def:beta*/Act() { }
}

public static class Factory
{
    public static Alpha /*def:factory*/Make() => new();
}

public static class Compact { public static void /*def:compact-int*/Same(int x) {} public static void /*def:compact-string*/Same(string x) {} }
