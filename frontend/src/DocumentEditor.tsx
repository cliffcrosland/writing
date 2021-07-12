import React, { useState } from 'react';
import './DocumentEditor.css';
import { importWasm } from './importWasm';

function DocumentEditor(props: any) {
  const { InputEventParams, DocumentEditorModel, Selection } = importWasm();

  const clientId: string = props.clientId;

  const [value, setValue] = useState('');
  const [documentEditorModel, _] = useState(() => {
    return DocumentEditorModel.new(clientId);
  });
  const [selection, setSelection] = useState(Selection.new(0, 0));
  const [history, setHistory] = useState(new Array<string>());
  
  function captureSelection(event: any) {
    const newSelection = Selection.new(
      event.target.selectionStart, 
      event.target.selectionEnd
    );
    documentEditorModel.setSelection(newSelection.clone_selection());
    setSelection(documentEditorModel.getSelection());
  }

  function onInput(event: any) {
    event.preventDefault();

    console.log("onInput");
    console.log("event.nativeEvent.inputType", event.nativeEvent.inputType);
    console.log("event.nativeEvent.data", event.nativeEvent.data);
    console.log("event.target.value", event.target.value);
    console.log("event.target.selectionStart", event.target.selectionStart);
    console.log("event.target.selectionEnd", event.target.selectionEnd);
    console.log();

    const inputEventParams = InputEventParams.new(
      event.nativeEvent.inputType,
      event.nativeEvent.data,
      event.target.value,
      Selection.new(event.target.selectionStart, event.target.selectionEnd)
    );

    documentEditorModel.processInputEvent(inputEventParams);

    setValue(documentEditorModel.getValue());
    setSelection(documentEditorModel.getSelection());
    setHistory(documentEditorModel.getHistory());
  }

  return (
    <div className="DocumentEditor">
      <div className="DocumentEditor-controls">
        <h2>Client ID: {clientId}</h2>
        <textarea 
          className="DocumentEditor-text"
          onDragStart={captureSelection}
          onKeyDown={captureSelection}
          onSelect={captureSelection}
          onInput={onInput}
          value={value}
        ></textarea>
        <div className="DocumentEditor-selection">
          { selection.toString() }
        </div>
        <div className="DocumentEditor-history">
          <ul className="DocumentEditor-historyList">
            {history.map((historyItem, i) =>
              <li key={i}>{historyItem}</li>
            ).reverse()}
          </ul>
        </div>
      </div>
    </div>
  );
}

export default DocumentEditor;
