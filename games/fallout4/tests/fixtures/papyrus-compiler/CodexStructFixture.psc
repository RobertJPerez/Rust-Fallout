ScriptName CodexStructFixture extends ScriptObject

Struct Payload
    Int identifier
    String label
EndStruct

Function ReadPayload(Payload value) Global
    Int copiedIdentifier = value.identifier
    value.label = "updated"
EndFunction

Function ReadArray(Int[] values) Global
    Int arrayCount = values.Length
    Int first = values[0]
    values[0] = arrayCount
EndFunction

