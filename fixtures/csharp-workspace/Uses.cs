namespace Probe;

public static class Uses
{
    public static void Exercise(IWorker contract, Worker worker, Derived derived, dynamic unknown)
    {
        Targets./*site:int*/Pick(1);
        Targets./*site:string*/Pick("x");
        _ = Targets./*site:generic*/Identity(1);
        worker./*site:extension*/Touch();
        _ = new /*site:constructor*/Worker();
        contract./*site:interface*/Run();
        contract./*site:default-interface*/Default();
        worker./*site:virtual*/Run();
        derived./*site:override*/Run();
        worker./*site:field*/Count = 1;
        worker./*site:property*/Value = 2;
        Action /*def:delegate*/callback = Targets./*site:delegate-target*/Callback;
        /*site:delegate*/callback();
        unknown./*site:dynamic*/Missing();
        _ = "😀"; Targets./*site:unicode*/Pick(1);
        Factory./*site:factory*/Make()./*site:cross-file*/Act();
        Compact./*site:compact-int*/Same(1);
        Compact./*site:compact-string*/Same("x");
        Library.Api./*site:cross-project*/Work();
        _ = Targets./*site:constant*/Limit;
        _ = nameof(Targets.OnlyNamed);
    }
}
