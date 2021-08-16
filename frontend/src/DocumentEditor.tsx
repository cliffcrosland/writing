import React, {
  useEffect,
  useRef,
  useState
} from 'react';
import './DocumentEditor.css';
import { importWasm } from './importWasm';

const DEBUG_LOGGING = false;
const PERFORMANCE_LOGGING = true;
const Z_KEY_CODE = 90;

function DocumentEditor(props: any) {
  const { InputEventParams, DocumentEditorModel, JsBackendApi, JsSelection } = importWasm();

  const textAreaElem: any = useRef(null);
  const [title, setTitle] = useState('Untitled Document');
  const [loaded, setLoaded] = useState(false);
  const [documentEditorModel, _] = useState(() => {
    return DocumentEditorModel.new(props.docId);
  });
  const [debugSelection, setDebugSelection] = useState(JsSelection.new(0, 0));
  const [debugLines, setDebugLines] = useState(new Array<string>());

  // Load the document metadata, sync contents.
  useEffect(() => {
    if (loaded) return;
    async function loadDocument() {
      try {
        const getDocumentPromise = JsBackendApi.getDocument(props.docId);
        const syncPromise = documentEditorModel.sync();
        const [getDocumentResponse, _] = await Promise.all([getDocumentPromise, syncPromise]);
        setTitle(getDocumentResponse.document.title);
        setLoaded(true);
        syncDocumentValueAndSelection();
      } catch (e: any) {
        console.error(e);
      }
    }
    loadDocument();
  });

  // Periodically run sync.
  useEffect(() => {
    let intervalId = setInterval(() => {
      console.log('Running sync...');
      sync();
    }, 1000);
    return function () {
      clearInterval(intervalId);
    };
  });

  function captureSelection(event: any) {
    const newSelection = JsSelection.new(
      event.target.selectionStart,
      event.target.selectionEnd
    );
    documentEditorModel.setSelection(newSelection.clone_selection());
    setDebugSelection(documentEditorModel.getSelection());
  }

  function onKeyDown(event: any) {
    captureSelection(event);

    const isUndoOrRedo = (event.keyCode == Z_KEY_CODE && event.ctrlKey);
    if (!isUndoOrRedo) return;
    event.preventDefault();
    const inputType = event.shiftKey ? 'historyRedo' : 'historyUndo';
    const inputEventParams = InputEventParams.new(
      inputType, null, null,
      JsSelection.new(event.target.selectionStart, event.target.selectionEnd)
    );
    updateFromInputEvent(inputEventParams);
  }

  function onInput(event: any) {
    event.preventDefault();

    if (DEBUG_LOGGING) {
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
      JsSelection.new(event.target.selectionStart, event.target.selectionEnd)
    );

    updateFromInputEvent(inputEventParams);
  }

  function updateFromInputEvent(inputEventParams: any) {
    if (PERFORMANCE_LOGGING) performance.mark("updateFromInputEventStart");
    documentEditorModel.updateFromInputEvent(inputEventParams);
    if (PERFORMANCE_LOGGING) performance.mark("updateFromInputEventEnd");
    syncDocumentValueAndSelection();
  }

  function syncDocumentValueAndSelection() {
    if (!textAreaElem.current) return;
    if (PERFORMANCE_LOGGING) performance.mark("getValueStart");
    textAreaElem.current.value = documentEditorModel.getValue();
    if (PERFORMANCE_LOGGING) {
      performance.mark("getValueEnd");
      performance.measure("updateFromInputEvent", "updateFromInputEventStart", "updateFromInputEventEnd");
      performance.measure("getValue", "getValueStart", "getValueEnd");
      performance.getEntriesByType("measure").forEach((entry) => {
        console.log(entry.name, `${entry.duration} ms`);
      });
      performance.clearMarks();
      performance.clearMeasures();
    }
    const selection = documentEditorModel.getSelection();
    textAreaElem.current.selectionStart = selection.start;
    textAreaElem.current.selectionEnd = selection.end;
    setDebugSelection(selection);
    if (DEBUG_LOGGING) {
      setDebugLines(documentEditorModel.getDebugLines());
    }
  }

  async function sync() {
    try {
      await documentEditorModel.sync();
      syncDocumentValueAndSelection();
      if (DEBUG_LOGGING) {
        setDebugLines(documentEditorModel.getDebugLines());
      }
    } catch (e) {
      console.error("Error syncing with server:", e);
    }
  }

  return (
    <div className="DocumentEditor">
      {!loaded ?
        <div>Loading...</div> :
        <div className="DocumentEditor-controls">
          <h1>{title}</h1>
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
                <button onClick={sync}>Sync</button>
              </div>
            </div>
            <ul className="DocumentEditor-revisionsList">
              {debugLines.map((line, i) =>
                <li key={i}>{line}</li>
              )}
            </ul>
          </div>
        </div>
      }
    </div>
  );
}

export default DocumentEditor;
