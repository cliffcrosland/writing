import React, {
  useEffect,
  useRef,
  useState
} from 'react';
import './DocumentEditor.css';
import DocumentValueChunk from './DocumentValueChunk';
import { importWasm } from './importWasm';
import { logPerformance } from './utils/performance';

const DEBUG_LOGGING = false;
const Z_KEY_CODE = 90;

function DocumentEditor(props: any) {
  const { InputEventParams, DocumentEditorModel, JsBackendApi, JsSelection } = importWasm();

  const textAreaElem: any = useRef(null);
  const [title, setTitle] = useState('Untitled Document');
  const [loaded, setLoaded] = useState(false);
  const [documentEditorModel, _] = useState(() => {
    return DocumentEditorModel.new(props.docId);
  });
  const [chunkMetas, setChunkMetas] = useState<Array<any>>([]);
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
        syncModelToView();
      } catch (e: any) {
        console.error(e);
      }
    }
    loadDocument();
  });

  // Periodically run sync.
  useEffect(() => {
    let intervalId = setInterval(() => {
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
    logPerformance('updateFromInputEvent', () => {
      documentEditorModel.updateFromInputEvent(inputEventParams);
    });
    syncModelToView();
  }

  function syncModelToView() {
    if (!textAreaElem.current) return;
    const value = logPerformance('getValue', () => documentEditorModel.getValue());
    if (textAreaElem.current.value !== value) {
      textAreaElem.current.value = value;
    }
    const selection = documentEditorModel.getSelection();
    if (textAreaElem.current.selectionStart !== selection.start ||
        textAreaElem.current.selectionEnd !== selection.end) {
      textAreaElem.current.selectionStart = selection.start;
      textAreaElem.current.selectionEnd = selection.end;
    }
    setDebugSelection(selection);
    if (DEBUG_LOGGING) {
      setDebugLines(documentEditorModel.getDebugLines());
    }
    const ids = documentEditorModel.getChunkIds();
    const versions = documentEditorModel.getChunkVersions();
    if (ids.length === versions.length) {
      const chunkMetas = new Array(ids.length);
      ids.forEach((id: any, i: any) => {
        chunkMetas[i] = { id: ids[i], version: versions[i] };
      });
      setChunkMetas(chunkMetas);
    }
  }

  async function sync() {
    try {
      await documentEditorModel.sync();
      syncModelToView();
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
          <div className="DocumentEditor-chunks">
            {
              chunkMetas.map((chunkMeta) =>
                <DocumentValueChunk
                  key={chunkMeta.id}
                  id={chunkMeta.id}
                  version={chunkMeta.version}
                  model={documentEditorModel}
                  />
              )
            }
          </div>
        </div>
      }
    </div>
  );
}

export default DocumentEditor;
