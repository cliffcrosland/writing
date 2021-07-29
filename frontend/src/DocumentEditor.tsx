import React, { useRef, useState } from 'react';
import './DocumentEditor.css';
import { importWasm } from './importWasm';

const VERBOSE_LOGGING = false;
const Z_KEY_CODE = 90;

function DocumentEditor(props: any) {
  const { InputEventParams, DocumentEditorModel, Selection } = importWasm();

  const textAreaElem: any = useRef(null);
  const [documentEditorModel, _] = useState(() => {
    return DocumentEditorModel.new(props.orgId, props.docId, props.userId);
  });
  const [debugSelection, setDebugSelection] = useState(Selection.new(0, 0));
  const [debugRevisions, setDebugRevisions] = useState(new Array<string>());
  
  function captureSelection(event: any) {
    const newSelection = Selection.new(
      event.target.selectionStart, 
      event.target.selectionEnd
    );
    documentEditorModel.setSelection(newSelection.clone_selection());
    setDebugSelection(documentEditorModel.getSelection());
  }

  function onKeyDown(event: any) {
    captureSelection(event);

    const isUndoOrRedo = (event.keyCode == Z_KEY_CODE && event.ctrlKey)
    if (!isUndoOrRedo) return;
    const inputType = event.shiftKey ? 'historyRedo' : 'historyUndo';
    const inputEventParams = InputEventParams.new(
      inputType, null, null,
      Selection.new(event.target.selectionStart, event.target.selectionEnd)
    );
    updateFromInputEvent(inputEventParams);
  }

  function onInput(event: any) {
    event.preventDefault();

    if (VERBOSE_LOGGING) {
      console.log("onInput");
      console.log("event.nativeEvent.inputType", event.nativeEvent.inputType);
      console.log("event.nativeEvent.data", event.nativeEvent.data);
      console.log("event.target.value", event.target.value);
      console.log("event.target.selectionStart", event.target.selectionStart);
      console.log("event.target.selectionEnd", event.target.selectionEnd);
      console.log();
    }
    const inputType = event.nativeEvent.inputType;
    if (inputType === 'historyUndo' || inputType === 'historyRedo') {
      return;
    }

    const inputEventParams = InputEventParams.new(
      event.nativeEvent.inputType,
      event.nativeEvent.data,
      event.target.value,
      Selection.new(event.target.selectionStart, event.target.selectionEnd)
    );

    updateFromInputEvent(inputEventParams);
  }

  function updateFromInputEvent(inputEventParams: any) {
    documentEditorModel.updateFromInputEvent(inputEventParams);

    textAreaElem.current.value = documentEditorModel.getValue();
    const selection = documentEditorModel.getSelection();
    textAreaElem.current.selectionStart = selection.start;
    textAreaElem.current.selectionEnd = selection.end;
    setDebugSelection(selection);
    setDebugRevisions(documentEditorModel.getDebugRevisions());
  }

  async function submitNextRevision() {
    try {
      await documentEditorModel.submitNextRevision();
    } catch (e) {
      console.error("Error submitting next revision:", e);
    }
    setDebugRevisions(documentEditorModel.getDebugRevisions());
  }

  return (
    <div className="DocumentEditor">
      <div className="DocumentEditor-controls">
        <textarea 
          ref={textAreaElem}
          className="DocumentEditor-text"
          onDragStart={captureSelection}
          onSelect={captureSelection}
          onKeyDown={onKeyDown}
          onInput={onInput}
        ></textarea>
        <div className="DocumentEditor-selection">
          { debugSelection.toString() }
        </div>
        <div className="DocumentEditor-revisions">
          <div className="DocumentEditor-submitRevision">
            <div>
              <button onClick={submitNextRevision}>Submit Next Revision</button>
            </div>
          </div>
          <ul className="DocumentEditor-revisionsList">
            {debugRevisions.map((revision, i) =>
              <li key={i}>{revision}</li>
            )}
          </ul>
        </div>
      </div>
    </div>
  );
}

export default DocumentEditor;
