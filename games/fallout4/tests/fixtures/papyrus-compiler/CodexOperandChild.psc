ScriptName CodexOperandChild extends CodexOperandParent

Function CallStatic(Int value) Global
    CodexOperandParent.StaticTarget(value, "static")
EndFunction

Function CallMethod(CodexOperandParent receiver) Global
    receiver.MethodTarget(17, 2.5)
EndFunction

Function ParentTarget(Int value)
    Parent.ParentTarget(value)
EndFunction

Function CallBranch(Int value) Global
    if value > 0
        CodexOperandParent.StaticTarget(value, "true")
    else
        CodexOperandParent.StaticTarget(0, "false")
    endif
EndFunction
