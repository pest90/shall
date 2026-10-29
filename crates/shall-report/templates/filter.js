// Opens linked graphs, and filters the level tables and the requirement
// details. Without JavaScript the form stays hidden and everything is
// shown.
(function () {
  "use strict";
  // The element the hash names; none for a hash that is not encoded
  // properly.
  function hashTarget() {
    var id;
    try {
      id = decodeURIComponent(location.hash.slice(1));
    } catch (error) {
      return null;
    }
    return id ? document.getElementById(id) : null;
  }
  // A link to a collapsed graph, e.g. from a tile, opens it, also when the
  // same link is followed again.
  function openTarget() {
    var target = hashTarget();
    if (target && target.tagName === "DETAILS") {
      target.open = true;
    }
  }
  window.addEventListener("hashchange", openTarget);
  document.addEventListener("click", function (event) {
    var link = event.target.closest && event.target.closest("a[href^='#']");
    if (link && link.getAttribute("href") === location.hash) {
      openTarget();
    }
  });
  openTarget();
  var form = document.getElementById("filter");
  if (!form) {
    return;
  }
  form.hidden = false;
  var items = document.querySelectorAll("[data-search]");
  var levels = document.querySelectorAll("section[id^='level-']");
  var count = document.getElementById("filter-count");
  function value(name) {
    return form.elements[name].value;
  }
  function apply() {
    var status = value("status");
    var coverage = value("coverage");
    var type = value("type");
    var tag = value("tag");
    var provided = value("provided");
    var words = value("search").toLowerCase().split(/\s+/).filter(Boolean);
    var shown = 0;
    var total = 0;
    items.forEach(function (item) {
      var data = item.dataset;
      var match =
        (!status || data.status === status) &&
        (!coverage || data.coverage === coverage) &&
        (!type || data.type === type) &&
        (!tag || data.tags.split("\n").indexOf(tag) >= 0) &&
        (!provided || data.provided === provided) &&
        words.every(function (word) {
          return data.search.indexOf(word) >= 0;
        });
      item.hidden = !match;
      // Rows and details come in pairs; count each requirement once.
      if (item.tagName === "TR") {
        total += 1;
        if (match) {
          shown += 1;
        }
      }
    });
    // A level whose rows are all filtered out is hidden as a whole.
    levels.forEach(function (level) {
      var rows = level.querySelectorAll("tr[data-search]");
      level.hidden =
        rows.length > 0 &&
        Array.prototype.every.call(rows, function (row) {
          return row.hidden;
        });
    });
    count.textContent = shown + " of " + total + " requirements shown";
  }
  // A link to a requirement the filter hides clears the filter first.
  function reveal() {
    var target = hashTarget();
    if (target && target.closest("[hidden]")) {
      form.reset();
      setTimeout(function () {
        apply();
        target.scrollIntoView();
      }, 0);
    }
  }
  window.addEventListener("hashchange", reveal);
  document.addEventListener("click", function (event) {
    var link = event.target.closest && event.target.closest("a[href^='#']");
    if (link && link.getAttribute("href") === location.hash) {
      reveal();
    }
  });
  form.addEventListener("input", apply);
  form.addEventListener("submit", function (event) {
    event.preventDefault();
  });
  form.addEventListener("reset", function () {
    setTimeout(apply, 0);
  });
  apply();
})();
